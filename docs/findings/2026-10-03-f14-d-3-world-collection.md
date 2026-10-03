# F14-D.3: the `world` collection of the retail baseline inventory

Date: 2026-10-03. Task: F14-D.3 (#486, "Populate the world collection of the
retail baseline inventory"), follow-up of F14-D.2 (#389) and F14-D.1 (#388).
Capability: `retail` (the installation at `$CS_GAME_DIR`, one language, 1033).
Evidence report: `docs/findings/evidence/F14-D.3.json`.

## What this stage adds

`docs/contracts/IDENTITY-CONTENT.md` requires "world groups and variants" as a
catalog collection. `retail_baseline` declared no `ContentKind::World` row at
all, so the report's `collections` object had no `world` entry. It now populates
the **group** half of that collection:

* one `ContentKind::World` row per world group that the **producing stage's own
  classifier** can name from original bytes — F14-D.1's
  `reader_dirs::classify`, reading each `ZBD/<group>/zrdr.zbd` member index
  through `cs_assets::zbd::audit_containers`. No reader was derived for this
  task, and no row is derived from a directory name.

Each row carries:

| field | value |
| --- | --- |
| `id` | `world/<group>`, the group directory lowercased (`world/c1c`) — the derivation `cs_content::campaign_bindings` already uses for a mission binding's `world` row (`ContentId::from_source(ContentKind::World, world_group)`), so the two cannot disagree |
| `display_name` | the group's own directory spelling (`ZBD/C1C`), outside identity |
| `origin` | `Origin::Installation` over the shared reader's checked span (`offset 0`, the archive's length, the installation fingerprint) |
| `dependencies` | one `Static` edge onto the **inventory row of the reader archive whose member index named it**, claim `f14.d.3.baseline.world_reader`, class `observed_tool` |
| `parse_state` / `normalize_state` | `Unparsed` / `NotNormalized` — no member of a shared reader is decoded at this stage |
| `fingerprint` | the SHA-256 production discovery measured for that archive |
| `readiness` / `unsupported_reasons` | `Unavailable` with `NotParsed`: a world row is a located, counted world group, never a claim that it loads |

The `World` collection is **not** launchable (`ContentKind::is_launchable`), so no
world row is declared a root and the coverage denominator did not move.

## Measured on this installation

Eight rows, ids `world/c1`, `world/c1b`, `world/c1c`, `world/c2`, `world/c2b`,
`world/c3`, `world/c4`, `world/c5`, all `Origin::Installation`, all pointing at
the `install_file/zbd_2f_<group>_2f_zrdr.zbd` inventory row of their own group.
The collection record reports `rows: 8`,
`gaps.declared_group_without_reader: 0` and no diagnostic. The whole report is
now 346 rows: 228 inventoried files, 24 campaign missions, 53 program rows,
8 + 21 scenario rows, 4 mode rows and 8 world rows; `launchable` is still 53
(24 + 8 + 21), `coverage.reachable` still `3 × 53` and `unresolved_references`
still 0. `coverage.unreachable_by_kind` now also carries `world: 8`.

The retail acceptance test measures the expected groups by walking `ZBD/`
directly rather than trusting the classifier, and asserts that the eight rows,
the eight world-group-reader classifications and the eight directories are the
same set.

## Decisions a reviewer should check

1. **A row needs the member index, so an unreadable reader yields no row.**
   `classify` refuses a directory whose archive cannot be listed, so a group
   whose shared reader is unreadable produces no `world` row at all. It is
   counted in the collection record's `declared_group_without_reader` gap and
   stays named in `Baseline::unrecognized_program_dirs`, exactly as F14-D.2
   reports an unreadable mode table. When no group at all can be classified the
   record carries a `CollectionStatus::diagnostic` naming the reader pattern and
   the declared groups. A synthetic test drives both states.
2. **The identity is the group directory, not an enumeration order.** The row
   keys off the directory the shared reader sits in, lowercased — the same
   derivation `campaign_bindings` uses — so a mission's world identity, the
   `WorldId` namespace and this row are one id. The retail test compares the row
   ids against an independent directory walk, which is what makes "eight" a
   measurement.
3. **The span covers the archive, not one member.** The classifier reads the
   member *index*, so the bytes that named the group are the archive's index
   rather than a member payload. The span is therefore `offset 0` and the
   archive's whole length, and the finding says so instead of implying a
   member-level precision. A later stage that reads a member payload can narrow
   it.
4. **No world row is reachable from a declared root.** Nothing points at a world
   yet, so the rows stay counted in `coverage.unreachable_by_kind` and
   `unreachable_needing_classification`. The same state F14-D.2 left the mode
   rows in, and it is the honest reading: a world row is a located group, not a
   reachable scene.
5. **A world *group* is what this stage produces; a *variant* is not.** Nothing
   in a shared reader's member index names a variant, sector or object
   instance, so minting variant rows would be the name-only guess rule 4 rejects.
   `IDENTITY-CONTENT`'s collection requirement stays half-populated and the
   remainder is named in the evidence report's limitations.

## Files

- `crates/cs_content/src/catalog/baseline.rs`: the module doc's world bullet,
  `CLAIM_WORLD_READER`, `WORLD_READER_PATTERN`, `world_rows`, `world_group_key`,
  `world_row`, the call in `retail_baseline` and the `CollectionStatus::source`
  doc's pattern case.
- `crates/cs_content/tests/accept_f14_d_3_world_collection.rs` (new): four
  synthetic tests and one retail test (`accept_f14_d_3_`).
- `crates/cs_content/tests/accept_f14_d_baseline.rs`: the retail row-count
  assertion states the new total (files + 2 per launchable + mode rows + world
  rows) instead of filtering the world rows out.
- `crates/cs_content/tests/evidence_report_f14_d_3.rs` (new): the evidence
  harness.

No wiring edit was needed: `catalog::baseline` was already public.

## Not known / not claimed

1. **A world row is a group, not a variant, and nothing in it is decoded.**
   Affected content: every variant, sector, object instance and mesh of all
   eight groups. Resolving tasks: **#392** (decode the GameZ node array),
   **#436** (F18-E, placement and the stored vertex unit) and the
   scene/mesh collection task created from #389.
2. **The rows are unreachable** from the declared roots, so they stay counted as
   unreachable unknowns. Affected content: the reachability accounting of the
   world collection. Resolving task: F18-E / the scene stage, when a mission or
   scenario row points at the world it runs in.
3. **A group must be declared and listable.** The classifier only accepts a leaf
   whose parent group the campaign walk declares and whose shared reader lists
   `templates.zrd` + `cam_anim.zrd` with no per-mission member and no
   `mis_anim.zbd` beside it. A world group the original shipped without a
   campaign mission would therefore yield no row (the gap count would say so).
   Measured on this installation: eight of eight groups carry at least one
   mission. Resolving task: the discovery stage if a capture ever shows a
   mission-less group.
4. **A world-group reader is not launchable content, and this stage did not move
   the denominator.** The eight rows are counted and reachable-by-nothing, not
   added to the 53. Resolving task: none; this is the intended contract
   (`ContentKind::is_launchable`).
5. **Only six of the required collections are populated.** Not yet populated
   here: world variants, scene nodes/meshes/materials (#487, blocked on #392),
   factions and paint masks (#488), airframes and flight equipment (#489),
   sounds/music/dialogue/video (#490), stunts, scrapbook items and legacy custom
   planes (#491). The report's `collections` and `collection_status` objects
   state what exists today.
6. **Nothing here is `verified_original`.** The rows are agent observations over
   the installation's bytes with `observed_tool` provenance; the ceiling for an
   agent review is `checked`.

## Sources

`specs/F18-world-geometry-terrain-water-and-traversable-interiors.md`,
`specs/F06-zbd-families-reader-archives-and-sound-containers.md`,
`docs/contracts/IDENTITY-CONTENT.md`, `docs/contracts/CLI-EVIDENCE.md`,
`docs/findings/2026-09-29-f14-d-retail-baseline-inventory.md`,
`docs/findings/2026-10-02-f14-d-1-reader-archive-directories.md`,
`docs/findings/2026-10-02-f14-d-2-multiplayer-rules-collection.md`,
`docs/findings/2026-09-30-f18-d-world-group-audit-and-gpu-capture.md`,
`crates/cs_content/src/catalog/baseline.rs` and
`crates/cs_content/src/catalog/reader_dirs.rs`. No original bytes, no member
content beyond filenames, and no binaries are committed.