# F14-D.1: classifying the reader-archive directories F14-D left outside the denominator

Date: 2026-10-02. Task: F14-D.1 "Classify the retail reader-archive directories
F14-D leaves outside the denominator" (follow-up of F14-D, #60). Capability:
**`retail`** (read-only `$CS_GAME_DIR`); no `gpu`, `audio`, `human_play` or
`human_review` capability was used, nothing was run, rendered or played, and
the claim stays `implemented` (at most `checked` after review).

## Files and the observable failure

- `crates/cs_content/src/catalog/reader_dirs.rs` (new): `ReaderDirRole`,
  `ClassifiedReaderDir` and `classify`, the rules below.
- `crates/cs_content/src/catalog/baseline.rs`: `retail_baseline` lists each
  leftover reader archive's own member index (the F06-C producer,
  `cs_assets::zbd::audit_containers` over a mounted session), classifies it,
  gives every launchable scenario directory a `Script` row and an
  `IaScenario`/`MultiplayerScenario` row declared launchable, and reports
  `classified_reader_dirs`. `unrecognized_program_dirs` now holds only what no
  rule classifies.
- `crates/cs_content/src/catalog/mod.rs` (wiring): `pub mod reader_dirs;` and a
  doc paragraph.
- `crates/cs_content/tests/accept_f14_d_baseline.rs`: the retail denominator
  test expects the new count; a new synthetic test (`accept_f14_d_1_…`) builds
  valid version-one reader archives. `crates/cs_content/tests/evidence_report_f14_d.rs`
  follows the new denominator and takes `CS_EVIDENCE_TASK_ID`.

**Observable failure without the change:** the 29 scenario directories are not
in the denominator, so `launchable` is 24 and any "every launchable scenario"
claim reads a denominator that omits them. The retail test counts the `IA<n>` /
`MP<n>` directories with a separate `std::fs` walk (not the classifier) and
fails if the baseline's launchable count is not `24 + 8 + 21`.

## What each directory is (measured from the original data)

The 38 `unrecognized_program_dirs` of F14-D, each read through its own
version-one member index (names only; no record was decoded):

| Directories | Count | Shape observed | Role |
| --- | --- | --- | --- |
| `ZBD/<group>/IA1` | 8 (every world group) | `zrdr.zbd` + `mis_anim.zbd`, the campaign-mission shape; 12 members: the per-mission set (`map`, `aiv`, `objectives`, …) plus **`ia.zrd`** | instant-action scenario |
| `ZBD/<group>/MP1`…`MP3` | 21 (MP1 and MP3 in all 8 groups, MP2 in C1, C2, C3, C4, C5) | same two files; 10–11 members: the per-mission set plus **`net.zrd`** | multiplayer scenario |
| `ZBD/<group>/zrdr.zbd` | 8 | no `mis_anim.zbd` beside it; 25–70 members of shared world data (`templates.zrd`, `cam_anim.zrd`, …) and **none** of the per-mission members | world-group reader, not launchable |
| `ZBD/zrdr.zbd` | 1 | 221 members of install-wide definitions, including `instantaction.zrd` and `multiplayer_setup.zrd`, and none of the per-mission members | shared reader, not launchable |

Corroboration outside the directories (names only, from the top-level reader):
`ia_escape.zrd`, `Loading.zrd` and `escape.zrd` carry the instant-action
briefing message ids (`MSG_BRF_IA…`); `multi1_*`, `multi2_*`,
`mp1_fighter_release.zrd` and `mp2_fighter_release.zrd` define the two
multiplayer zeppelins. F56-A independently measured the same 21 `MP<n>` slots
(`docs/findings/2026-10-02-f56-a-multiplayer-catalog.md`).

**Rule (code, `reader_dirs::classify`):** a leaf named `IA<n>` is an
instant-action scenario only if its reader also lists `ia.zrd`; a leaf named
`MP<n>` is a multiplayer scenario only if its reader also lists `net.zrd`; both
need a `mis_anim.zbd` beside the archive, the three per-mission members and a
declared world group. The name never decides alone. A world-group reader
needs `templates.zrd` + `cam_anim.zrd`, no per-mission member and no
`mis_anim.zbd`; the shared reader needs `instantaction.zrd` +
`multiplayer_setup.zrd` with the same absences. Anything else — including an
archive that cannot be listed — stays in `unrecognized_program_dirs`.

## Denominator

| | Before | After |
| --- | --- | --- |
| launchable rows (`launchable`) | 24 | **53** = 24 missions + 8 `ia_scenario` + 21 `multiplayer_scenario` |
| rows | 276 | 334 (+29 scripts, +29 scenarios) |
| `unrecognized_program_dirs` | 38 | 0 |
| `classified_reader_dirs` | — | 38 (29 launchable, 9 not) |

Every new row is `Origin::Installation` with a span over the reader archive
(`offset 0`, the file's length, the installation fingerprint) and that file's
SHA-256; its ids are `ia_scenario/<group>-ia1`,
`multiplayer_scenario/<group>-mp<n>` and `script/<group>-<leaf>-zrdr`. All are
`unavailable` (nothing is decoded), so `ready` stays 0 and `is_retail_ready`
stays false. The edge provenance is `observed_tool`, never `verified_original`.

## Recorded unknowns (not guessed)

- **"Launchable" is structural.** A scenario directory is declared launchable
  because it is shaped like (and named and populated like) a directory the game
  starts; whether and how the original launches it was not observed (no
  original run exists, and `retail` is file access only). F49/F56 and an
  original capture can overturn a row.
- **Presets inside one IA1.** `ia.zrd` is not decoded here; it appears to hold
  several selectable mission types (key names such as mission type, zeppelin
  type, player plane and wingman count are visible). How many player-selectable
  presets one IA1 directory yields is **unmeasured**, so the denominator unit
  is the directory and may undercount instant-action configurations.
  Resolving task: F49.
- **Slot-to-mode binding.** Four multiplayer modes are named in the string
  table (F56-A) but which mode a slot is launched under is unknown there too,
  so a launchable multiplayer unit may be a slot × mode pair. Resolving task:
  F56-B / the slot program decoding.
- **World rows.** The world-group and shared readers are classified, not
  turned into `World` rows: worlds are a collection #389 (F14-D.2) owns.

These limits are quoted in the evidence report's `review.method` and gate any
claim that the denominator is "every launch configuration of the original
game".

## Tests (`accept_f14_d_` prefix; `accept_f14_d_1_` for this task)

- `cs_content::catalog::reader_dirs::tests::accept_f14_d_1_scenario_roles_need_a_name_and_a_corroborating_member`
  and `…accept_f14_d_1_shared_readers_are_classified_and_not_launchable`: the
  rules, including the refusals (name without member, member without name,
  missing `mis_anim.zbd`, unknown group).
- `accept_f14_d_1_reader_directories_are_classified_from_their_member_index`
  (synthetic, valid archives): roles, rows, edges, denominator 3, the
  uncorroborated `MP2` stays unrecognized, byte-stable report.
- `accept_f14_d_retail_baseline_inventory_is_complete_and_never_synthetic`
  (retail): `24 + 8 + 21`, independent directory walk, 38 classified, 0
  unrecognized, `3 × 53` reachable rows, no synthetic row.

## Sources

`specs/F14-…` (non-negotiable behavior 4), `docs/contracts/IDENTITY-CONTENT.md`,
`docs/findings/2026-09-29-f14-d-retail-baseline-inventory.md`,
`docs/findings/2026-10-02-f56-a-multiplayer-catalog.md`, the F06-D reader
member listings (`cs-inspect zbd-audit`), and the owner's installation read
through `cs_assets` (no original bytes, names beyond member filenames, or
binaries are committed).
