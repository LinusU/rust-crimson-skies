# F14-D.8: the stunt and scrapbook collections of the retail baseline inventory, and why the legacy custom-plane collection stays empty

Date: 2026-10-03. Task: #491 "Populate the stunt, scrapbook and legacy
custom-plane collections of the retail baseline inventory" (follow-up of #389 /
F14-D.2). Feature sheet:
`specs/F14-canonical-content-catalog-and-dependency-closure.md`, stage
`### F14-D`. Shared contract: `docs/contracts/IDENTITY-CONTENT.md`; evidence
contract `docs/contracts/CLI-EVIDENCE.md`. Required capability: **`retail`**,
used read-only. `gpu` and `audio` were available and **not used**: nothing is
rendered or played, no original run happened, and nothing here claims
`verified_original`.

## What this stage changes about the F14-D verdict

`docs/contracts/IDENTITY-CONTENT.md` requires "stunts and scrapbook rewards" and
"legacy custom-plane resources" as catalog collections. The baseline inventory
held no `ContentKind::Stunt`, `ScrapbookItem` or `CustomPlane` row at all, so
its report had no such entries. This stage settles all three:

* **`stunt` is populated** — one row per fly-through danger-zone target of an
  instant-action scenario the installation's own `ia.zrd` marks `stunt_flying`;
* **`scrapbook_item` is populated** — one row per `Mission_Spread_Item` record of
  the shared archive's `ASSETS/SCRAPBOOK.CSV` member;
* **`custom_plane` stays empty, deliberately** — no row and no collection record,
  because no installation byte is known to name one (see below).

The baseline report over the owner's installation now reads:

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
| `airframe` | 11 | `ZBD/interp.zbd`'s loading scripts |
| `sound` | 4 951 | F14-D.7's ZBD sound family (measured by that stage; this stage only has to account for it, below) |
| **`stunt`** | **45** | **the `stunt_flying` instant-action scenarios** |
| **`scrapbook_item`** | **461** | **`GOSDATA/ASSETS/crimson.rof`'s `ASSETS/SCRAPBOOK.CSV`** |

The coverage denominator did **not** move: **53** declared roots (24 campaign
missions plus 29 scenario directories), unchanged. Neither a stunt nor a
scrapbook item is launchable content, so the 506 new rows are counted as
unreachable unknowns in `coverage.unreachable_by_kind.stunt` (45) and
`coverage.unreachable_by_kind.scrapbook_item` (461) rather than entering the
denominator. Over the merged tree — F14-D.7's 4 951 sound rows landed after the
stage-level completeness total was last widened — the catalog holds **6 009**
rows, **159** reachable, **5 850** unreachable, **0** ready, **0** unresolved
references. This stage's own contribution is the 506 new rows; the rest is the
collection set the later stages already measured.

## Where the stunts come from

The measurement is T463's
(`docs/findings/2026-10-02-t463-stunt-encoding-and-gate-geometry.md`), re-read
here through the producing stage's own readers rather than restated:

* an instant-action scenario is the reader archive
  `ZBD/<world group>/IA<n>/zrdr.zbd`. Its own `ia.zrd` member names the mode; the
  value `stunt_flying` marks a stunt scenario (`cs_content::stunts::
  scenario_mission_type` over `decode_zrd`).
* its `targets.zrd` member is a list of objectives. The fly-through danger-zone
  objectives are the ones whose `category_label` names `MSG_OBJ_DZ` and whose
  `help_label` names `MSG_OBJ_FLYTHROUGH`
  (`cs_content::stunts::scenario_fly_through_targets`).
* the identity is the scenario directory's own key plus the target's **own
  scenario-local zone label** — `stunt/c1b-ia1-dz1` — never a file name, never
  the objective's line number and never the localized description, which is a
  message key and not text this catalog may copy.
* the row is located by the `targets.zrd` member's own checked extent (offset
  and length from the container discovery) with the digest of exactly those
  bytes, and its one `Static` edge points at the inventory row of the archive it
  was read from.

Measured on the owner's installation: **54** fly-through danger-zone targets in
total across eight scenarios, **45** of them in a `stunt_flying` scenario
(`c1b` 5, `c2` 9, `c4` 14, `c5` 17 — four scenarios). The remaining **9** live
in the two `dogfight_squadron` scenarios (`c1` 5, `c3` 4); `c1c` and `c2b`
(`zeppelin_run`) author none. Those nine are **not** dropped and **not** turned
into stunt rows: the same objective shape backs a different mission type, so
they are counted in the collection record under the stable gap
`non_stunt_fly_through_targets`. The `45/54` split is exactly what the bytes
say; it is an encoding measurement, not a claim about which mode the running
game calls a "stunt".

### What a stunt row does not know

The scenario bytes state an objective, never a rule. The five things the bytes
do not carry are each an explicit `UnsupportedReason::Unknown` with its own
claim, so a consumer sees what is missing instead of a designed default:

| claim id | the fact that is unknown |
| --- | --- |
| `f14.d.8.stunt_direction_rule` | the heading or sequence of passes that completes the gate |
| `f14.d.8.stunt_clearance_rule` | the rim margin or altitude a completion must keep |
| `f14.d.8.stunt_reward` | what a completion pays |
| `f14.d.8.stunt_repeat_policy` | whether a second pass pays again |
| `f14.d.8.stunt_geometry` | the world box of the detection zone the target names |

The gate geometry **is** measured elsewhere: T463's survey joined every one of
the 54 labels to a world box in `cs_app::world::triggers`. But that join lives
in `cs_app`, over one specific installation's world containers, and
`cs_content` cannot reach it; a retail byte offset copied into the catalog would
be stale on any other installation. So the catalog records the unknown rather
than a number, and the join stays where it was measured. A sixth reason,
`MissingRuntimeConsumer`, says the row has no consumer at all yet.

## Where the scrapbook items come from

F12-D (`docs/findings/2026-10-02-f12-d-installation-wide-configuration-account.md`)
measured the installation's configuration account: the shared ROF container
`GOSDATA/ASSETS/crimson.rof` carries a routed `KeyedList` member
`ASSETS/SCRAPBOOK.CSV`, **35 154** bytes decoded (7 639 stored), at offset
52 437, with **461** `Mission_Spread_Item` records, each **16** fields, none
untyped and none off-kind. This stage reads that member through the production
ROF mount (`cs_assets::rof::mount_rof_into`) and the production keyed-list
reader (`cs_content::config::ConfigDocument`) and keys each row by the record's
**own entry key**, never its line number.

* the row's span is the **decoded** member's extent at the measured offset, with
  the digest of the decoded bytes. A stored digest would be a different thing
  and is *not* used; `ConfigDocument::read` refuses a byte length that disagrees
  with the span, so a stale length fails loudly.
* the identity is the entry key encoded through the same injective byte encoder
  the inventory uses (`install_key_bytes`), so a key that is not valid UTF-8 can
  never be merged with another.
* an entry the documented schema does not cover is counted in the collection
  record under `entry_not_a_scrapbook_item` rather than becoming a guessed row.
  On the owner's installation that gap is absent: every parsed entry is a
  `Mission_Spread_Item`.

A scrapbook row is `Parsed` + `NotNormalized`: the record is read, but nothing
normalizes its image, cell, page or localization fields, so no downstream
consumer can build a spread yet.

## Why the legacy custom-plane collection stays empty

The task's own rule refuses "no row from a file name", and F64-A measured that
no installation file is known to reference an importable legacy aircraft:
its `LEGACY_LAYOUT_INVENTORY[CustomAircraft]` entry has `referenced_by: &[]` and
evidence `Unknown`
(`docs/findings/2026-10-01-f64-a-legacy-import-inventory-and-contracts.md`).
Since no byte supports an identity, any `custom_plane` row could only be
invented from a file name — exactly what AGENTS rule 4 forbids. So the stage
writes **no** `custom_plane` row and, following the F14-D.6 precedent for a
family with no located source, **no** `custom_plane` `CollectionStatus` either:
a record must name the file its rows come from, and no such file has been
measured. The refusal is proved by the acceptance test and the evidence harness
(no row, no collection record, no `custom_plane` key in the report) and the
layout question is carried into the evidence report's limitations and the
follow-up task created from #491.

## What this stage adds

- `crates/cs_content/src/catalog/baseline.rs`: the `stunt` and `scrapbook_item`
  collections — `STUNT_SCENARIO_PATTERN` and the five public stunt claim
  constants plus the private `CLAIM_STUNT_TARGET` edge claim,
  `SCRAPBOOK_CONTAINER` / `SCRAPBOOK_MEMBER` / `SCRAPBOOK_ITEM_CLAIM`, the
  `stunt_rows` and `scrapbook_rows` builders and the private `install_key_bytes`
  encoder (the existing `install_file_key` now delegates to it), plus the two
  `collection_status` records. The module needs no new dependency: `cs_formats`
  and `cs_types` were already there, and `cs_assets::rof` and
  `cs_content::config` are already used by neighbouring collections.
- `crates/cs_content/tests/accept_f14_d_8_stunt_scrapbook_collections.rs` (new):
  eight synthetic `accept_f14_d_8_*` tests plus one `#[ignore = "requires
  CS_GAME_DIR"]` retail test.
- `crates/cs_content/tests/evidence_report_f14_d_8.rs` (new): the evidence
  harness, deliberately **not** named with the acceptance prefix.
- `crates/cs_content/tests/accept_f14_d_baseline.rs`: the stage-level retail
  completeness total now accounts for the 45 stunt and 461 scrapbook rows (the
  same repair F14-D.6 made for the airframes; the equality is not weakened and
  the rows are not filtered out). It also accounts for F14-D.7's 4 951 sound
  rows: that collection landed on `main` after the total was last widened and
  did not add itself to the equality, so on the rebased tree
  `accept_f14_d_retail_baseline_inventory_is_complete_and_never_synthetic`
  failed before this stage touched it. The test now counts the sound rows as
  **F14-D.7's** collection — it does not restate that stage's own number — so
  the merged tree's completeness equality holds. This is a two-line accounting
  repair inside this stage's owner path (`crates/cs_content/tests/`), recorded
  here because it fixes a defect that was already on `main`; the F14-D.7 stage's
  own acceptance test still owns the sound identities.
- `docs/findings/evidence/F14-D.8.json` (new): the validated acceptance report.

## Design decisions

- **The rows come from the producing stage's own readers.** The stunts use
  `cs_formats::script_raw::discover_container`,
  `cs_content::stunts::scenario_mission_type` and
  `scenario_fly_through_targets`; the scrapbook items use
  `cs_assets::rof::mount_rof_into` and `cs_content::config::ConfigDocument`. No
  rule is derived here that the producing stages have not already measured, so a
  partner stage's fix improves these rows rather than diverging from them.
- **Every row's span is measured, not written down.** The stunt span is the
  `targets.zrd` member's extent from the archive discovery; the scrapbook span
  is the decoded member's extent from the ROF source. A copied retail offset
  would be stale on any other installation.
- **A collection that reads nothing is a named gap, not an absent collection.**
  A missing `crimson.rof`, an archive that does not mount, a missing
  `SCRAPBOOK.CSV` member, a member that does not decode and a member that is not
  the keyed-list table each yield `rows: 0` plus a `diagnostic` naming the
  source. An instant-action scenario whose members do not decode is skipped, and
  an installation whose scenarios yield no stunt at all gets a diagnostic naming
  `stunt_flying`. A report reader can tell "this installation declares none"
  from "this installation declares something I could not read".
- **No collection record for the custom-plane family with no located source**
  (see above). The emptiness is carried by this note, the evidence report's
  limitations and the follow-up task.

## What the retail run measured

`$CS_GAME_DIR`, production discovery and fingerprint, the production
`retail_baseline` that the acceptance test and the evidence harness both run,
and two independent derivations (a manifest walk of the scenario archives for
the stunts and a second parse of the scrapbook member). Evidence artifact
`baseline-report.json`, hashed by `docs/findings/evidence/F14-D.8.json`. **No
string text from the installation is reproduced in this file**; the rows are
identified by their catalog ids and counts.

| quantity | value |
| --- | --- |
| `stunt` rows | **45** |
| stunt scenarios | **4** (`c1b`, `c2`, `c4`, `c5`) |
| non-stunt fly-through targets (`non_stunt_fly_through_targets`) | **9** (`c1` 5, `c3` 4) |
| stunt collection diagnostic | `null` |
| `scrapbook_item` rows | **461** |
| scrapbook entries the schema does not cover (`entry_not_a_scrapbook_item`) | none |
| decoded scrapbook member digest | `28b5144c54120f52c36717a3f1e094cb75845ecb1f854334a5686d5f6c6af5c1` |
| `custom_plane` rows / collection records | **0 / 0** |
| rows that are closure roots | **0** |
| rows whose edge resolves inside the inventory | **506 of 506** |
| unresolved references | **0** |
| declared roots (the denominator) | **53**, unchanged |
| coverage ready | **0** |
| `accept_f14_d_8_*` tests executed | **9** (8 synthetic, 1 retail), all passing |

## Test inventory

| `accept_f14_d_8_*` test | Covers | Fails when |
| --- | --- | --- |
| `a_stunt_scenario_becomes_one_row_per_fly_through_target` (cs_content/tests/accept_f14_d_8_stunt_scrapbook_collections.rs) | the mapping arm over a synthetic installation: a `stunt_flying` scenario with two fly-through targets becomes two rows in canonical id order (`c1-ia1-dz1`, `c1-ia1-dz2`), each located by the `targets.zrd` member's span with its own decoded digest, `parsed`/`not_normalized`/`unavailable`, the five ordered claim ids plus `missing_runtime_consumer`, each unknown carrying why, one static edge onto `install_file/…/zrdr.zbd` with `observed_tool` provenance at the row's own span, the row fingerprint equal to the member digest, both rows sharing the same member span; the record's pattern source, language, rows and no gap; the report's `stunt` count and id; and byte-stability for the same installation | a row is keyed by the objective's description instead of its zone label, a display name is invented, an unknown is dropped or replaced by a value, the edge moves off the archive's inventory row, the fingerprint stops matching, or the report stops being deterministic |
| `a_non_stunt_scenario_is_a_gap_not_a_row` | a `dogfight_squadron` scenario with a fly-through target: **no** row, `non_stunt_fly_through_targets: 1`, a diagnostic naming `stunt_flying` | a non-stunt objective becomes a stunt row, or its gap is dropped |
| `a_target_that_is_not_a_danger_zone_is_not_a_stunt` | a `stunt_flying` scenario whose only objective is a rearm base: **no** row, `rows: 0` | an objective is selected by position rather than by its own measured labels |
| `the_zone_label_is_the_identity_not_the_position` | two scenarios naming the same zone label yield two distinct rows | the same label in two scenarios collides into one identity |
| `the_scrapbook_table_becomes_one_row_per_item` | the mapping arm: two `Mission_Spread_Item` records become two rows keyed by their own entry keys, located by the decoded member's extent and digest, with `not_normalized` and one static edge onto the archive's inventory row, the record's `SCRAPBOOK_CONTAINER` source and `entry_not_a_scrapbook_item: 1` for the one `B`-letter layout record, and byte-stability | a row is keyed by line number, the span uses the stored digest instead of the decoded one, a display name is invented, the layout record becomes a row, or the report stops being deterministic |
| `an_absent_or_unreadable_scrapbook_is_a_named_gap` | (a) a missing `crimson.rof` and (b) a non-ROF file at its spelling: both `rows: 0` with a diagnostic naming the source, and the mission root intact | a refused archive is reported as an empty reading, or its diagnostic is dropped |
| `the_new_collections_are_not_launchable` | `Stunt`/`ScrapbookItem`/`CustomPlane` are not launchable; the roots are only the mission and the scenario; `launchable_count == 2`; `unreachable_by_kind.stunt == 1`, `.scrapbook_item == 2` | the denominator moves when a collection is populated, or a row becomes a closure root |
| `custom_planes_have_no_row_and_no_collection_record` | no `CustomPlane` row, no `CustomPlane` collection record and no `custom_plane` key in the report | a custom plane is fabricated from a file name |
| `retail_the_installation_declares_its_stunt_and_scrapbook_collections` (`#[ignore = "requires CS_GAME_DIR"]`) | **the real installation**: 45 stunt rows equal to a second walk of the same scenarios from four distinct archives, the nine non-stunt targets as a gap, each row's five ordered claim ids and edge onto the archive; 461 scrapbook rows with the decoded member digest `28b5144c…`, `not_normalized` and the archive edge; no custom-plane row or record; no new root; and the report's `stunt: 45`, `scrapbook_item: 461`, no `custom_plane` | the corpus changes, a scenario stops being found or a non-stunt target becomes a row, the scrapbook count drifts, a row is promoted to a root, or the report claims completeness |

The evidence harness (`evidence_report_f14_d_8_writes_the_acceptance_report`,
deliberately **not** named with the task prefix) derives
`private/evidence/F14-D.8/acceptance.json` and `baseline-report.json` from the
recorded acceptance log, production discovery of `$CS_GAME_DIR`, the production
baseline over that installation, and two **independent derivations**: a manifest
walk that re-finds every `IA<n>/zrdr.zbd` and counts the `stunt_flying`
fly-through targets itself (never reading `classified_reader_dirs` or
`retail_baseline`), and a second parse of the `ASSETS/SCRAPBOOK.CSV` member
through the production ROF mount and keyed-list reader. It refuses a stale
`CS_CANDIDATE_TREE` and a missing log, and writes a failing report when the
acceptance run failed.

## Sensitivity check

Each mutation below was applied to `crates/cs_content/src/catalog/baseline.rs`,
the acceptance file was run, and the file was restored.

| # | Mutation | Killed by |
| --- | --- | --- |
| 1 | the `stunt_rows` call in `retail_baseline` is replaced by an empty row set | the stunt mapping test (identity list), the non-stunt gap test, `the_zone_label_is_the_identity_not_the_position` and the not-launchable test (`unreachable_by_kind.stunt`) |
| 2 | the `scrapbook_rows` call in `retail_baseline` is replaced by an empty row set | the scrapbook mapping test (identity list and record `rows`) and the not-launchable test (`unreachable_by_kind.scrapbook_item`) |
| 3 | the stunt row is keyed by the objective's description instead of its zone label | the stunt mapping test and `the_zone_label_is_the_identity_not_the_position`, on the identity lists |
| 4 | a scrapbook row's span uses the stored member digest instead of the decoded member digest | the **retail** test, on `span.member_sha256()`. The synthetic fixture's member is stored uncompressed, so both digests coincide there and the synthetic arm does not catch it; the retail test pins the decoded digest `28b5144c…` and fails |
| 5 | the five stunt unknowns are replaced by a single `not_normalized` | the stunt mapping tests, on `row.unsupported_codes()` and the ordered claim list |
| 6 | the collection's `gaps` are cleared | the non-stunt scenario test (`non_stunt_fly_through_targets`) and the scrapbook test (`entry_not_a_scrapbook_item`) |

The retail test additionally pins the 45 identities, the four archives, the 461
items and the five claim ids against the real installation, so mutations 3 and 5
fail there too — at the cost of a ~2 min run over the installation, which is why
the fast synthetic arms carry the same checks.

## Unknowns and limitations (all recorded, none guessed)

- **A stunt row is an identity and a location, not a playable stunt.** The five
  facts above (direction, clearance, reward, repeat, geometry) stay explicit
  unknowns on every row. **Affected content:** every stunt flight and its scoring
  in F42. **Resolving tasks:** F42 (stunts, fame photos and optional achievement
  events), F26 (the calibration probes) and F38 (native behavior bindings) —
  T463 measured that the original's own gate and reward semantics reach its stunt
  screen through native callbacks this engine has not decoded.
- **A scrapbook row is an identity and a parsed record, not a built spread.** The
  record is `NotNormalized`; nothing normalizes its image, cell, page or
  localization fields. **Affected content:** every scrapbook spread, its
  thumbnails and its captions in F47. **Resolving task:** F47 (scrapbook
  records, mementos and mission replay).
- **The legacy `custom_plane` collection has no row and no collection record.**
  The layout is a research question: the identity and container format of a
  legacy aircraft must be *measured* before any row can exist, and a row from a
  file name is exactly what the task forbids. **Affected content:** the whole of
  F64 (legacy custom aircraft and optional save import). **Resolving task:** the
  follow-up task created from #491, which must first measure the layout.
- **The nine `dogfight_squadron` fly-through targets are a counted gap, not
  stunts.** The same objective shape backs two mission types, and only the
  scenario's own `ia.zrd` says which; the distinction is F37/F42's to make.
  **Affected content:** the stunt/objective distinction of `c1` and `c3`.
- **Nothing references a stunt or a scrapbook item yet**, so the 506 rows are
  unreachable from the declared roots and stay counted in
  `coverage.unreachable_by_kind` and `unreachable_needing_classification`.
  **Affected content:** the reachability accounting of the two collections.
  **Resolving task:** the stage that lets a scenario or a mission row point at
  the stunt it awards or the scrapbook item it unlocks.
- **The scenario idiom is a claim with its own provenance**, shipped here
  (`cs_content::catalog::baseline`) and falsifiable only by the pinned row count.
  **Affected content:** the stunt collection's reachability for an installation
  whose scenarios spell the mission type differently. **Resolving tasks:** F07-D
  and F13 (the opcode and mission-language stages) and F42's shipped consumer.
- **No row claims a runtime consumer and no member program is decoded**, so the
  coverage accounting reports 0 ready and every launchable row as unsupported.
  **Affected content:** every campaign mission's and scenario directory's
  readiness. **Resolving tasks:** F37 (mission IR) and F38 (native behavior
  bindings).
- **Evidence class.** The rows are derived from original bytes by production
  readers, which makes them `observed_tool`. `retail` is file access, not proof
  that the original executable ran; the `.zrd` and keyed-list semantics this
  stage reads are the corpus's own spelling and were not verified against the
  running engine. No original run happened and nothing claims
  `verified_original`.
- **Independent review is outstanding for this work.** AGENTS.md asks a
  different agent instance or model with a fresh context to review format and
  catalog work. The report's `review.identity` names the implementer and says a
  reviewing agent must replace it; until that review happens nothing here is
  above `checked`, and no agent review replaces the owner's human approval.
- **Fixture scope.** The eight unignored tests are synthetic and newly authored;
  only the `#[ignore]`d retail test reads original data. Nothing derived from it
  beyond ids, counts, offsets and digests is committed.

## Sources used

- `specs/F14-canonical-content-catalog-and-dependency-closure.md` (stage
  `### F14-D`, non-negotiable behavior 4, AC02/AC04).
- `specs/F42-stunts-fame-photos-and-optional-achievement-events.md`,
  `specs/F47-scrapbook-records-mementos-and-mission-replay.md`,
  `specs/F64-legacy-custom-aircraft-and-optional-save-import.md`.
- `docs/contracts/IDENTITY-CONTENT.md` (the required collections and the rule
  that a collection cannot exclude failed entries).
- `docs/contracts/CLI-EVIDENCE.md` and `schemas/evidence.schema.json`.
- `docs/findings/2026-10-02-t463-stunt-encoding-and-gate-geometry.md` (the
  measured 54 / 45 split, the scenario members and the five unmeasured rules).
- `docs/findings/2026-10-02-f12-d-installation-wide-configuration-account.md`
  (the 461 `Mission_Spread_Item` records, the decoded member's offset, lengths
  and digest).
- `docs/findings/2026-10-01-f64-a-legacy-import-inventory-and-contracts.md`
  (the measured legacy inventory, `referenced_by: &[]`, evidence `Unknown`).
- `docs/findings/2026-10-03-f14-d-6-airframe-collection.md` (the collection
  record pattern and the "no source, no record" precedent).
- `crates/cs_formats::script_raw` (`discover_container`),
  `crates/cs_content::stunts` (`decode_zrd`, `scenario_mission_type`,
  `scenario_fly_through_targets`), `crates/cs_assets::rof` (`mount_rof_into`),
  `crates/cs_content::config` (`ConfigDocument`, `RecordSchema`) and
  `crates/cs_assets::install` (production discovery and the fingerprints).
- The owner's installation, read-only: the `ZBD/<group>/IA<n>/zrdr.zbd` reader
  archives and `GOSDATA/ASSETS/crimson.rof`'s `ASSETS/SCRAPBOOK.CSV` member.
