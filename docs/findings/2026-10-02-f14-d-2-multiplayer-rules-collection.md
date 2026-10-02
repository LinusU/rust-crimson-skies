# F14-D.2: the `multiplayer_rules` collection of the retail baseline inventory

Date: 2026-10-02. Task: F14-D.2 (#389, "Populate the remaining content
collections of the retail baseline inventory"), follow-up of F14-D (#60).
Capability: `retail` (the installation at `$CS_GAME_DIR`, one language, 1033).
Evidence report: `docs/findings/evidence/F14-D.2.json`.

## What this stage adds

`cs_content::catalog::baseline::retail_baseline` populated three of the
collections `docs/contracts/IDENTITY-CONTENT.md` requires (install files,
campaign missions, mission programs). It now populates a fourth:

* one `ContentKind::MultiplayerRules` row per multiplayer mode the
  installation's string image names, read by **stage F56-A's own parser** —
  `cs_content::multiplayer::discover_modes` over the string rows
  `cs_content::config::StringCatalog` reads out of `strings.dll` — never by a
  reader derived for this task.

Each row carries:

| field | value |
| --- | --- |
| `id` | `multiplayer_rules/mode.name-<name string id>` (F56-A's identity: the name's own string id, never its position in a walk) |
| `display_name` | the mode's localized name as the string table spells it |
| `origin` | `Origin::Installation` with the checked `RT_STRING` block span the name was read from (container `strings.dll`, no member key, a non-zero offset inside the image) |
| `dependencies` | one `Static` edge onto the `install_file/strings.dll` inventory row, provenance class `observed_tool` |
| `parse_state` / `normalize_state` | `Parsed` / `NotNormalized` (the row holds no quantity to convert) |
| `fingerprint` | the SHA-256 production discovery measured for `strings.dll` |
| `readiness` / `unsupported_reasons` | `Unavailable`, with one `UnsupportedReason::Unknown` per rule F56-A could not resolve, each keeping F56-A's own claim id (`f56.mode.name-<id>.<rule>`) and reason |

A collection record — `Baseline::collection_status` — reports what the
producing parser did per collection: kind, source, language, row count, the
gaps the parser keeps visible, the id that ended its measured walk, and a
diagnostic when it holds no rows. It renders into the report as
`"collection_status"`.

## Measured on this installation

Four rows, ids `mode.name-7011` .. `mode.name-7014`, all
`Origin::Installation`, all pointing at the `install_file/strings.dll` row.
The collection record reports `rows: 4`, no gaps and `boundary_id: 16680` —
the first briefing block outside the multiplayer family, which is what makes
"four" a measurement rather than an assumption (F56-A's finding,
`docs/findings/2026-10-02-f56-a-multiplayer-catalog.md`). The retail
acceptance test pins the ids, the spans, the edge, the digest and the boundary;
it asserts no original mode or briefing **text**, only measured ids, counts
and digests.

## Decisions a reviewer should check

1. **The denominator did not move.** `ContentKind::MultiplayerRules::is_launchable`
   is false, so the coverage denominator is still the 24 campaign missions.
   `accept_f14_d_retail_baseline_inventory_is_complete_and_never_synthetic`
   (F14-D) was updated because the **row count** changed (280 rows = 228 files
   + 48 campaign rows + 4 mode rows), and it now asserts the mode rows
   explicitly instead of filtering them out.
2. **An unreadable or absent mode table is reported, not fatal.** A missing
   `strings.dll`, a file that is not a PE resource image, or a table the F56-A
   walk refuses produces no rows **and** a `collection_status` diagnostic that
   names the file and the refusal. Refusing the whole inventory would hide the
   collections that do read; emitting a row for bytes nobody read would be the
   filename guess rule 4 rejects. A synthetic test drives both states.
3. **A read failure of an inventoried file is an error**
   (`BaselineError::Read`). Installation discovery hashes every regular file
   before this walk, so a file that cannot be read afterwards means the
   installation is no longer readable as inventoried — not a silently empty
   collection.
4. **A named mode with no briefing is a gap, not a row.** F56-A pairs a name
   with a briefing by its own comparison key; an unpaired name has no identity
   this engine may mint, so it is counted in `collection_status.gaps`
   (`name_without_briefing`, `briefing_without_name`) and the paired rows are
   still built. A synthetic test drives that state too.

## Files

- `crates/cs_content/src/catalog/baseline.rs`: `CollectionStatus`,
  `Baseline::collection_status`, `multiplayer_rules_rows`, `mode_row`,
  `MODE_STRING_IMAGE`, `MODE_STRING_LANGUAGE`, `BaselineError::Read`, the
  `collection_status` report section and two unit tests.
- `crates/cs_content/tests/accept_f14_d_2_multiplayer_rules.rs` (new): four
  synthetic tests and one retail test (`accept_f14_d_2_`).
- `crates/cs_content/tests/accept_f14_d_baseline.rs`: the retail row-count
  assertion above, and `collection_status` in the hand-built `Baseline`.
- `crates/cs_content/tests/evidence_report_f14_d_2.rs` (new): the evidence
  harness.

No wiring edit was needed: `catalog::baseline` was already public.

## Not known / not claimed

1. **A mode row is unavailable.** Every rule F56-A could not resolve is a
   typed unknown on the row (on this installation: all twelve labels for a mode
   whose team play the table does not state, eleven for the others). Resolving
   task: #476 (per-mode rule measurement against an original capture), then
   F56-B.
2. **The rows are unreachable** from the campaign roots — nothing references a
   mode yet — so they stay counted in `coverage.unreachable_by_kind` and
   `unreachable_needing_classification`. Resolving task: F56-B, when a
   scenario row points at the mode it runs.
3. **The collection is read in one language** (`LANG_ENGLISH_US`, the only one
   this installation carries). Another localization could hold a different run;
   `discover_modes` takes the language as input, so this is a decision, not a
   search.
4. **Only four of the required collections are populated.** Not yet populated
   here: worlds (F14-D.3, #486), scene nodes/meshes/materials (#487, blocked on
   #392), factions and paint masks (#488), airframes and flight equipment
   (#489), sounds/music/dialogue/video (#490), stunts, scrapbook items and
   legacy custom planes (#491). Instant-action and multiplayer **scenario**
   rows are task #388's work (in review at the time of writing), not this
   stage's.
5. **Nothing here is `verified_original`.** The rows are agent observations
   over the installation's bytes with `observed_tool` provenance; the ceiling
   for an agent review is `checked`.

## Reviewer notes

- Mutation probes to repeat: remove the `multiplayer_rules_rows` call from
  `retail_baseline` (the four synthetic tests fail), return
  `multiplayer_rules` rows from a hand-built list without the F56-A unknowns
  (the reason assertions fail), and set `CollectionStatus::diagnostic` to
  `None` on the unread paths (the two gap tests fail).
- The F14-D evidence copy (`docs/findings/evidence/F14-D.json`) records the
  F14-D candidate tree and quotes "only three collections are populated"; that
  text describes F14-D's own candidate, and this finding plus the F14-D.2
  evidence report supersede it for the multiplayer collection. It is left in
  place rather than edited, because rewriting a merged task's evidence copy
  would misstate the tree it was measured on.
