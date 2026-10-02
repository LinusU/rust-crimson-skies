# F09-PALETTE: the original faction paint palette and stored paint combinations

Date: 2026-10-02. Task: F09-PALETTE "Extract the original faction paint palette
and valid paint combinations from original data" (Rally #385; follow-up to
F09-D). Specification:
`specs/F09-bm-multilayer-liveries-and-paint-composition.md`, non-negotiable #4
("Extract palette choices and valid combinations from original data; hardcoded
colors from the Blender helper are research leads, not the authoritative
catalog"). Shared contract: `docs/contracts/IDENTITY-CONTENT.md` (`SourceSpan`).
Test prefix: `accept_f09_palette_`. Capabilities used: `retail` (read-only
`$CS_GAME_DIR`) plus ordinary build/test. Evidence:
`docs/findings/evidence/F09-PALETTE.json`; private artifacts stay in
`private/evidence/F09-PALETTE/`.

## What this closes

F09-D recorded, as a machine-readable unknown, that the only faction colors used
anywhere were S10's `FACTION_COLORS` research lead and that no original-data
palette had been extracted. This finding is the authoritative catalog for the
factions the original data actually stores: it names the member, the grammar,
the exact byte span of every color, decal and pattern, and the vehicle records
that carry them. S10's `FACTION_COLORS` is now a lead that these eleven palettes
can be compared against, not the source of truth.

## Files and the one observable failure (listed before editing)

- `crates/cs_content/src/livery.rs`: the content-layer production path for this
  stage — the `.zrd` decoder (`ZrdReader`, `ZrdNode`, `ZrdKind`, `ZrdFailure`,
  `parse_zrd`, `zrd_field_pairs`, `vehicle_record_nodes`), the extraction
  (`PaletteProvenance`, `PaletteColor`, `PaletteDecal`, `PaletteRecord`,
  `PaletteRefusal`, `FactionPalette`, `PaletteFinding`, `PaletteError`,
  `FactionPaletteCatalog::discover`, `extract_palette_record`, `palette_color`,
  `palette_decal`, `same_palette_values`), the seven inline
  `accept_f09_palette_*` tests and the inline ignored evidence harness.
- This file, `docs/findings/evidence/F09-PALETTE.json` and the private
  `palette-catalog.json` artifact.
- `crates/cs_formats`, `crates/cs_app`: **not touched**. This stage adds no BM
  layout or composition behavior; it reads a config member the F09 chain did not
  read before.

**One observable failure:** before this stage no production code read the paint
fields of `vehicle.zrd`; every faction color in the project came from S10's
research table. A wrong member offset, a wrong `.zrd` node count (a list of `N`
holds `N - 1` children, not `N`) or a fabricated palette would pass every earlier
test. With this stage, `accept_f09_palette_retail_faction_palettes_and_combinations`
fails on the first color, decal or span that does not match the original member,
and the synthetic tests fail if the decoder accepts a layout it should refuse.

## The `.zrd` grammar

`vehicle.zrd` is a little-endian node stream. Every node is a `u32` tag followed
by its payload:

| tag | kind | payload |
| --- | --- | --- |
| `1` | int | one `u32` |
| `2` | float | one `f32` (recognized, never read: no paint field is a float) |
| `3` | text | one `u32` byte length, then that many bytes |
| `4` | list | one `u32` count `N`, then **`N - 1`** child nodes |

The off-by-one is the one place a guess would have failed: a list's count word is
one greater than its child count. Measured against every `.zrd` member decoded
during the search — `vehicle.zrd`, `instantaction.zrd`, `multiplayer_setup.zrd`,
`player.zrd` and `player_setup.zrd`, plus `pilots.zrd`, `plane_props.zrd` and
`plane_splash.zrd` from the sibling `zrdr` group — each consumes its member
exactly, with zero trailing bytes. A decode that ends before the member and a
decode that overruns both fail: `parse_zrd` refuses a nonzero tail.

`vehicle.zrd`'s root is one list whose only child is the record list: alternating
`T3` record-name strings and `T4` record lists. Each record's own list alternates
`T3` field names and their values; the paint fields are `paint_pattern` (a
one-text list) and `paint_color1..3` / `paint_decal1..3` (three-element and
one-element int lists).

## Member provenance

| Property | Value |
| --- | --- |
| Container | `ZBD/zrdr.zbd` |
| Member | `vehicle.zrd` |
| Container offset | 1 397 861 |
| Length | 97 917 |
| Member SHA-256 | `d22cabb0038c6bde6481a38992729657a4de60b3d6dbf668c71d29545d671baf` |
| Reader | `cs_content::livery::FactionPaletteCatalog::discover` |

Every color, decal and pattern is exposed with a container-absolute
`SourceSpan` (`install_sha256`, `container_path = "ZBD/zrdr.zbd"`,
`member_key = "vehicle.zrd"`, `offset = member offset + node offset`,
`length`, `member_sha256`). A color is a `T4` list of three `T1` ints and its
span is 32 bytes (`T4`, count 4, three `T1`); a decal is a one-int list; a
pattern span covers the `paint_pattern` value list. Nothing in the production
API is a hardcoded color table — every value is decoded from the member at the
recorded span, and two records that store the same value at different offsets
carry different spans.

## The extracted palettes

Eleven paint patterns carry a complete color and decal triple. Colors are in
stored order; decals are `(paint_decal1, paint_decal2, paint_decal3)`. The
`palette_fingerprint` over every faction name, color byte and decal index is
`a221fa9b1583d1a92041bc325da21abbbb4e02331857d57b99e53d98fae8ed5d`.

| Faction | `paint_color1 / 2 / 3` | decals 1/2/3 | Records |
| --- | --- | --- | --- |
| `blackhat` | (177,130,66) / (119,74,43) / (66,39,15) | 21, 2, 2 | `bhatbrigand_2`, `bhatbrigand`, `bhatwarhawk`, `bhatgyro`, `bhatbrigand_5`, `bhatwarhawk_5` |
| `blake` | (149,163,195) / (89,114,159) / (233,228,240) | 21, 3, 3 | `blakebloodhawk`, `blakepeace_2`, `blakepeace`, `blakepeace_3` |
| `blckswan` | (23,23,21) / (48,47,39) / (196,193,186) | 21, 5, 5 | `bsfury`, `bsfury_5` |
| `british` | (177,130,66) / (48,47,39) / (255,255,255) | 21, 4, 4 | `britpeace`, `britbalmoral` |
| `cccp` | (57,64,68) / (223,0,41) / (245,211,0) | 21, 6, 6 | `rusdevastator` |
| `german` | (96,115,126) / (0,0,0) / (48,47,39) | 21, 13, 13 | `germanhellhound` |
| `hollywd` | (108,102,169) / (67,36,121) / (212,202,225) | 21, 10, 9 | `hkfirebrand` |
| `hughes` | (243,194,0) / (0,0,0) / (255,255,255) | 21, 11, 11 | `hakestrel`, `habloodhawk`, `hafury` |
| `medusas` | (95,125,143) / (41,14,21) / (141,137,93) | 21, 14, 14 | `medkestrel`, `medbrigand` |
| `sactrust` | (52,38,107) / (243,194,0) / (23,23,21) | 21, 15, 15 | `stihellhound`, `stihellhound_5` |
| `studio` | (32,90,167) / (255,255,255) / (0,0,0) | 21, 11, 11 | `secfury`, `secgyro` |

`paint_decal1` is `21` for every record; `paint_decal2 == paint_decal3` for every
faction except `hollywd`, whose decals are `21, 10, 9`. Every record that names
one of these patterns stores exactly one triple; records that name the same
pattern agree on both the colors and the decals. The two remaining paint-bearing
records, `devastator` and `wingman`, name pattern `player_fortune` but store no
color or decal triple; that gap is a `pattern_without_colors` finding, not a
fabricated palette.

## The valid combinations this stage exposes

The original `vehicle.zrd` stores, per vehicle record, one `paint_pattern` and
that record's accepted `paint_color1..3` / `paint_decal1..3`. That is the
`(faction, mask colors, decals)` combination each airframe actually carries, and
`FactionPalette::records` names the airframes it was measured on. The 11 factions
above cover 26 of the 28 paint-bearing records. What is **not** established here
is the paint shop's rule for combining a faction with an airframe the file does
not name — that belongs to F09-PAINTSHOP (Rally #485) and is recorded below. This
stage also does not interpret a decal index or map color slots to BM mask planes;
both are recorded unknowns, not assumptions.

## Tests

Six `accept_f09_palette_` tests, all in `crates/cs_content/src/livery.rs`, plus
one ignored evidence harness beside them. Every assertion is produced by
production code; each fails if the behavior it pins is removed. Five run on an
ordinary build (CI); the retail test and the harness are
`#[ignore = "requires CS_GAME_DIR"]` and fail loudly when run without it.

| Test | What it pins |
| --- | --- |
| `accept_f09_palette_extracts_the_faction_palette_and_spans` | a synthetic reader archive whose `vehicle.zrd` holds two painted records: the `T4` list decoder, the color/decal values, and the exact 32-byte color span (including the `T4` tag and count word) with `container_path = ZBD/zrdr.zbd`, `member_key = vehicle.zrd` and the member digest |
| `accept_f09_palette_refuses_unknown_factions_and_slots` | an unknown faction, an out-of-range color slot and an out-of-range decal slot are refused with the requested slot and the available count, never padded with a zero |
| `accept_f09_palette_refuses_a_member_that_is_not_the_observed_layout` | a member with an unknown node tag is refused with the decoder's own code, and an archive that does not declare `vehicle.zrd` is refused as a missing member (never read as an empty palette) |
| `accept_f09_palette_records_a_pattern_without_colors_as_a_finding` | a record that names a pattern but stores no colors is kept in the records, named by a `pattern_without_colors` finding, and never becomes a faction palette |
| `accept_f09_palette_groups_agreeing_records_and_flags_value_disagreement` | two records that name one pattern and store equal values at **different spans** are one faction with no finding; a real value disagreement is an `inconsistent_pattern_palette` finding |
| `accept_f09_palette_retail_faction_palettes_and_combinations` | the original member: the 11 sorted faction names, the 28 paint-bearing records, every pinned color and decal, each color's container-absolute span and member digest, the member offset/length/digest, the two `player_fortune` gaps as the only findings, and `player_fortune` never becoming a palette |
| `evidence_report_f09_palette_writes_the_acceptance_report` (ignored harness) | writes the acceptance report from the recorded suite, production discovery/fingerprint and the production catalog; it is **not** named `accept_f09_palette_*` so the task selection cannot pick it up |

Mutation checks, applied to the production code and reverted, with the tests
that caught each:

- comparing `PaletteColor`/`PaletteDecal` (which include the span) instead of
  their values → every same-pattern record looked inconsistent; the retail test
  saw 15 `inconsistent_pattern_palette` findings and the dedicated grouping test
  fails (this is the bug the first evidence run caught, see below);
- using `N` instead of `N - 1` for a list's child count → the decoder overruns
  `vehicle.zrd` and the retail test fails;
- a hand-altered member offset, length or digest constant → the retail test
  fails on the production reader's own numbers;
- dropping the `pattern_without_colors` branch → the finding test and the retail
  test fail.

## Recorded unknowns

Each names the affected content and the Rally task that resolves it, and each is
carried machine-readably in the evidence report's `unknowns` array.

1. **`player_fortune` has no stored palette.** The `devastator` and `wingman`
   records name the player's pattern but store no colors or decals. Affected
   content: the player paint on every airframe. Resolving task: F09-PAINTSHOP
   (Rally #485), which owns the paint-shop swatch source (`PAINT.SCRIPT` native
   callbacks 2236/2237/2229/2239, `LAYOUT.CSV` `[@Paint@]`).
2. **`BROADWAY` and `ITSTAXI` have no paint-bearing record.** Both have stock BM
   livery directories in the F09-D inventory but no `vehicle.zrd` record, so
   their colors are not in this source. Affected content: those two factions'
   colors on every composed livery. Resolving task: F09-PAINTSHOP (Rally #485).
3. **The decal index semantics are unmeasured.** Values range over 2..21 and
   `paint_decal1` is always 21; whether an index names a cell of the
   `PX_P_DECALS` sheet or an engine decal id is unknown. Affected content: every
   decal selection on every composed livery. Resolving task: F09-PAINTSHOP
   (Rally #485).
4. **The shade multipliers are unknown.** `LAYOUT.CSV`'s `[@Paint@]` shade
   dropdowns give counts (10 entries), not values; the values come from native
   callback 2236. Affected content: every shaded paint variant. Resolving task:
   F09-PAINTSHOP (Rally #485).
5. **The slot-to-mask mapping is inferred, not observed.** The three colors are
   exposed in stored order; that slot 1/2/3 corresponds to BM `Mask1`/`Mask2`/
   `Mask3` is not established from an original render. Affected content: the mask
   assignment of every composed livery. Resolving task: F17-D (composition
   agreement, the F09-D `retail_composition_agreement` boundary), with the
   paint-shop semantics from F09-PAINTSHOP (Rally #485).

## Evidence

This task used `retail` (read-only access to `$CS_GAME_DIR`), so
`docs/contracts/CLI-EVIDENCE.md` requires an acceptance report. It is
`docs/findings/evidence/F09-PALETTE.json`, written by the inline harness from a
real run: the recorded `cargo test --workspace --locked -- accept_f09_palette_
--include-ignored` log (6 tests, all pass), the production `install::discover` /
`fingerprint` / `content_fingerprint` of the installation, the production
`FactionPaletteCatalog::discover` over the original member, and the private
`palette-catalog.json` artifact (faction names, color/decal values and
container-absolute spans, the member digest and the palette fingerprint). No
original bytes, text or images are committed. The report's
`capabilities` are `["retail", "synthetic"]`; `claim` is `implemented` — a merge
awards `checked` at most and nothing here observed the original game running.

The report is validated with `tools/validate_evidence.py` **without**
`--require-pass`, for the reason the F12-G and F12-H reports give: the flag
rejects a report that lists unresolved issues, and this task's deliverable is
that five limitations stay recorded with the content they affect. Deleting them
to turn the flag green would be the thing the contract forbids. Concretely, on
this tree:

| Command | Result |
| --- | --- |
| `python3 tools/validate_evidence.py private/evidence/F09-PALETTE/acceptance.json --artifact-root private/evidence/F09-PALETTE` | 0 (`structurally_valid: true`, 2 artifacts, all six assertions `pass`) |
| `python3 tools/validate_evidence.py … --require-pass` | **3** (`Unresolved issues`) — expected: the five unknowns are the deliverable, not failed assertions |

The first evidence run is worth recording. It reported 15
`inconsistent_pattern_palette` findings that do not exist: the consistency check
compared whole `PaletteColor`/`PaletteDecal` values, which include the
provenance span, so every same-pattern record at a different offset looked
different. The original member shows all same-pattern records store identical
colors and decals. The comparison was changed to values only
(`same_palette_values`), the synthetic grouping test was added, and the report
was regenerated on the corrected tree (candidate tree
`38700addf43de3a5203e99f9648531c898c69885`, two `pattern_without_colors`
findings, palette fingerprint
`a221fa9b1583d1a92041bc325da21abbbb4e02331857d57b99e53d98fae8ed5d`).

## Boundaries

This stage extracts what the original data stores. It does not claim the paint
shop's full option set, the decal sheet, the shade tables or the original
renderer's mask mapping; those are the five recorded unknowns above and the
F09-PAINTSHOP and F17-D tasks. A `verified_original` claim about faction colors
still needs the owner's capture and is not made here.
