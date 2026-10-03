# F14-D.5: the `faction` and `paint_mask` collections of the retail baseline inventory

Date: 2026-10-03. Task: F14-D.5 (#488, "Populate the faction and paint-mask
collections of the retail baseline inventory"), follow-up of F14-D.2 (#389) and
F14-D.3 (#486). Capability: `retail` (the installation at `$CS_GAME_DIR`, one
language, 1033). Evidence report: `docs/findings/evidence/F14-D.5.json`.

## What this stage adds

`docs/contracts/IDENTITY-CONTENT.md` requires "blueprints and faction paint
masks" and "pilot/voice/faction relations" as catalog collections.
`retail_baseline` declared no `ContentKind::Faction` and no
`ContentKind::PaintMask` row at all. It now populates both from the **producing
stages' own parsers**:

* one `ContentKind::Faction` row per paint pattern the `vehicle.zrd` paint
  records of `ZBD/zrdr.zbd` **name in bytes**, read by
  `cs_content::livery::FactionPaletteCatalog::discover` (F09-PALETTE);
* one `ContentKind::PaintMask` row per `.bm` member of
  `GOSDATA/ASSETS/crimson.rof` that
  `cs_content::livery::StockLiveryCatalog::discover` read and verified through
  the production ROF and BM readers.

No reader was derived for this task and no row is derived from a file, member or
directory name. The rows are:

| collection | `id` | `display_name` | `origin` | `dependencies` | `parse_state` / `normalize_state` | `fingerprint` | `readiness` / `unsupported_reasons` |
| --- | --- | --- | --- | --- | --- | --- | --- |
| `faction` | `faction/<pattern>` (the byte-named `paint_pattern`) | the pattern | `Origin::Installation` over the pattern field's own checked span (`ZBD/zrdr.zbd`, member `vehicle.zrd`) | one `Static` edge onto the `install_file` inventory row of `ZBD/zrdr.zbd`, claim `f14.d.5.baseline.faction_pattern`, class `observed_tool` | `Parsed` / `NotNormalized` | the whole `vehicle.zrd` member's SHA-256 | `Unavailable` with `NotNormalized` |
| `paint_mask` | `paint_mask/<member spelling>` | the member spelling | `Origin::Installation` over the member's stored extent (`GOSDATA/ASSETS/crimson.rof`, member key, offset, stored length, member digest) | one `Static` edge onto the `install_file` inventory row of the ROF, claim `f14.d.5.baseline.paint_mask_member`, class `observed_tool` | `Parsed` / `NotNormalized` | the member's stored bytes SHA-256 | `Unavailable` with `NotNormalized` |

Neither kind is launchable (`ContentKind::is_launchable`), so neither collection
is a closure root and the coverage denominator did not move.

## Measured on this installation

195 rows: **184** `paint_mask` rows (the airframe library holds 184 `.bm`
members, all verified) and **11** `faction` rows, ids `faction/blackhat`,
`faction/blake`, `faction/blckswan`, `faction/british`, `faction/cccp`,
`faction/german`, `faction/hollywd`, `faction/hughes`, `faction/medusas`,
`faction/sactrust`, `faction/studio`. The collection records report
`paint_mask` `rows: 184`, `gaps` empty and no diagnostic; `faction` `rows: 11`,
`gaps.pattern_without_colors: 2` and no diagnostic.

## Why no mask-to-faction edge, and where the factions come from

The task's caution is that `StockLivery::from_file` derives a faction and an
airframe prefix from the **member name**, so building a faction row that way
would be filename-derived identity. The stage therefore takes factions from a
different, byte-backed source: the paint records of `ZBD/zrdr.zbd`'s
`vehicle.zrd` member, which name each faction as a `paint_pattern` **string
value**. That is what the installation names in bytes, and it is the only place
a faction identity is stored in a form a reader can read. The 2026-10-03
F09-PAINTSHOP finding (`docs/findings/2026-10-03-f09-paintshop-option-space-and-engine-internal-values.md`)
already records that `vehicle.zrd` names 12 paint patterns, that 11 store a
complete color/decal triple, and that `player_fortune` stores no palette.

The `GRAPHICS/<FACTION>/` **directory** a `.bm` member sits in is not the same
thing as a byte-named pattern, and the F09-PAINTSHOP finding records the
directory-to-pattern binding as engine-internal. This stage therefore does not
mint a member-to-faction edge from the member's directory: the mask row's
faction membership stays unknown and is named in the report's limitations and
the follow-up work. Minting it would be the name-only guess rule 4 rejects, and
the acceptance tests assert that no mask row carries a `faction` edge.

A paint record that names a pattern without a complete color triple is likewise
not a faction row: it is counted under the producing stage's own
`pattern_without_colors` code in the collection record's `gaps` (2 on this
installation, the `player_fortune` records), not minted from its name.

## Decisions a reviewer should check

1. **Factions are byte-named, masks are member-keyed.** A `faction` row's
   identity is a `paint_pattern` string read from the container's bytes, and a
   `paint_mask` row's identity is the escaped member spelling plus the member's
   stored extent. The acceptance tests compare both against values an
   independent reading of the fixture yields, and the retail test pins the 11
   pattern names and the 184 members.
2. **A missing or refused source is a diagnostic, not a silent empty
   collection.** When the palette archive or the airframe library is absent or
   does not read as the observed layout, the collection reports no row and a
   `CollectionStatus::diagnostic` naming the source, exactly as F14-D.2 and
   F14-D.3 report an unreadable table or reader. Two synthetic tests drive the
   missing and refused states.
3. **A member or pattern the producing stage rejects is a gap, not a row.** A
   `.bm` member that fails verification is counted under the verifier's own
   stable code (`not_a_stock_livery`, `empty_image`, `unexpected_eof`, ...); a
   pattern with no complete palette is counted under
   `pattern_without_colors`. A synthetic test drives both.
4. **The denominator does not move.** Neither kind is launchable, so no row is a
   closure root, and both collections stay counted as unreachable-by-kind. The
   synthetic and retail tests measure `launchable` against the roots rather than
   assuming it.
5. **The spans name the bytes that were read.** A mask row is located by the
   member's stored extent (container path plus member key plus offset/length and
   the member digest), and a faction row by the `paint_pattern` field's own
   offset inside `vehicle.zrd`. The acceptance test re-reads the fixture's bytes
   at the span to confirm the row names what it claims.

## Files

- `crates/cs_content/src/catalog/baseline.rs`: the module doc's faction/paint
  mask bullets, `CLAIM_FACTION_PATTERN`, `CLAIM_PAINT_MASK_MEMBER`,
  `PAINT_MASK_CONTAINER`, `FACTION_PALETTE_CONTAINER`, `faction_rows`,
  `paint_mask_rows` and their calls in `retail_baseline`.
- `crates/cs_content/tests/accept_f14_d_5_faction_and_paint_mask.rs` (new): six
  synthetic tests and one retail test (`accept_f14_d_5_`).
- `crates/cs_content/tests/evidence_report_f14_d_5.rs` (new): the evidence
  harness.

No wiring edit was needed: `catalog::baseline` was already public.

## Not known / not claimed

1. **A faction row is a paint palette, not a gameplay faction.** It carries the
   pattern, color triple and decals; no flag, relation, pilot roster or voice
   binding is decoded. Affected content: the F33 faction, relations and
   pilot-identity semantics. Resolving tasks: F33 and the pilot/voice/faction
   relations collection stage created from #389.
2. **Nothing binds a paint mask to a faction.** The `GRAPHICS/<FACTION>/`
   directory a `.bm` sits in is not byte-backed content (the 2026-10-03
   F09-PAINTSHOP finding records the directory-to-pattern binding as
   engine-internal), so no mask-to-faction edge is minted. Affected content:
   every paint mask's faction membership, which a runtime livery consumer needs.
   Resolving task: the livery-prefix-to-airframe binding work (#349-area) or an
   original capture that shows the binding.
3. **A mask row is a verified `.bm` member, not a rendered layer stack.** Its
   bytes are read and its stored extent checked, but the BM's layer composition
   and the paint pipeline are not decoded or normalized, and the airframe part
   the member names is not bound. Affected content: every mask's rendering and
   part binding. Resolving tasks: F09 (BM multilayer liveries) and F44
   (construction, loadouts and the paint editor).
4. **`player_fortune` is a named gap.** The 12 declared patterns include one
   that stores no color triple, so it is not a faction palette; it is counted in
   `gaps.pattern_without_colors` rather than minted from its name. Affected
   content: `player_fortune`'s starting swatches and shades, which the
   F09-PAINTSHOP finding records as engine-internal. Resolving task: the
   paint-shop value stage, if a capture or a later reader finds the palette.
5. **Only eight of the required collections are populated.** Not yet populated:
   world variants and scene nodes/meshes/materials (#487), airframes and flight
   equipment (#489), sounds/music/dialogue/video (#490), and stunts, scrapbook
   items and legacy custom-plane resources (#491). The report's `collections`
   and `collection_status` objects state what exists today.
6. **Nothing here is `verified_original`.** The rows are agent observations over
   the installation's bytes with `observed_tool` provenance; the ceiling for an
   agent review is `checked`.

## Sources

`specs/F09-bm-multilayer-liveries-and-paint-composition.md`,
`specs/F33-wingmates-factions-neutral-traffic-and-pilot-identity.md`,
`specs/F44-aircraft-construction-budgets-loadouts-and-paint-editor.md`,
`docs/contracts/IDENTITY-CONTENT.md`, `docs/contracts/CLI-EVIDENCE.md`,
`docs/findings/2026-10-03-f09-paintshop-option-space-and-engine-internal-values.md`,
`docs/findings/2026-09-29-f14-d-retail-baseline-inventory.md`,
`docs/findings/2026-10-02-f14-d-2-multiplayer-rules-collection.md`,
`docs/findings/2026-10-03-f14-d-3-world-collection.md`,
`crates/cs_content/src/catalog/baseline.rs` and
`crates/cs_content/src/livery.rs`. No original bytes, no member content beyond
filenames, and no binaries are committed.
