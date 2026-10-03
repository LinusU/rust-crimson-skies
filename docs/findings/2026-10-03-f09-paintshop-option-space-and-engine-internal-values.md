# F09-PAINTSHOP: the paint shop's declared option space, and which of its values are engine-internal

Date: 2026-10-03. Task: F09-PAINTSHOP "Extract the paint-shop palette, shade tables
and decal index semantics from original data" (Rally #485; follow-up to
F09-PALETTE, #385). Specification:
`specs/F09-bm-multilayer-liveries-and-paint-composition.md`, non-negotiable #4
("Extract palette choices and valid combinations from original data; hardcoded
colors from the Blender helper are research leads, not the authoritative
catalog"). Shared contract: `docs/contracts/IDENTITY-CONTENT.md` (`SourceSpan`).
Test prefix: `accept_f09_paintshop_`. Capabilities used: `retail` (read-only
`$CS_GAME_DIR`) plus ordinary build/test. Evidence:
`docs/findings/evidence/F09-PAINTSHOP.json`; private artifacts stay in
`private/evidence/F09-PAINTSHOP/`.

## The answer in one paragraph

The paint shop's **option space is in the original data**: the `[@Paint@]`
section of `ASSETS/LAYOUT.CSV` declares all ten paint-shop dropdowns, the number
of entries each offers (12 paint patterns, 18 colour swatches and 10 shades and
2 decals per paint slot) and the decal sheet (`PX_P_Decals.tga`, 50 frames).
The shop's option space, the decal sheet, the 12 = 12 closure between the
pattern list and the paint patterns `vehicle.zrd` names, and the fact that
BROADWAY and ITSTAXI are **not** paint patterns are all measured, with spans.
The paint shop's option space is also the *only* place the shop exists in
readable form, and **no field of any of its ten control records carries a value**:
not a colour, not a hex literal, not a name. The swatch palette, the shade table
and the pattern display names are therefore produced inside the engine image by
native callbacks, and no readable member of the installation stores them. The
decal index semantics are partly closed — the sheet's frame count bounds every
stored index — and the mapping from a stored index to a frame stays unknown.
`player_fortune` has no stored palette and is a shop choice; its starting
swatches and shades are engine-internal with the rest of the shop's values.

## Files and the one observable failure (listed before editing)

- `crates/cs_content/src/livery.rs`: the content-layer production path for this
  stage — the `F09-PAINTSHOP` section (`PAINT_SHOP_CONTAINER`,
  `PAINT_SHOP_LAYOUT`, `PAINT_SHOP_SECTION`, `PAINT_SHOP_DECAL_PANE`,
  `PAINT_SHOP_SLOTS`, `PaintShopRole`, `PAINT_SHOP_STEMS`,
  `PaintShopFieldCensus`, `PaintShopControl`, `PaintShopDecalSheet`,
  `PaintShopGap`, `PaintShopValue`, `PaintShopRefusal`, `PaintShopError`,
  `PaintShopCatalog::discover`, `PaintShopFinding`, `PaintShopCatalog::cross_check`,
  `paint_shop_stem`, `paint_shop_slot`, `paint_shop_control`,
  `paint_shop_decal_sheet`, `whole_number`, `paint_shop_gaps`), the five inline
  `accept_f09_paintshop_*` tests and the inline ignored evidence harness.
- `docs/findings/2026-10-03-f09-paintshop-option-space-and-engine-internal-values.md`
  (this file), `docs/findings/evidence/F09-PAINTSHOP.json` and the private
  `paint-shop-catalog.json` artifact.
- `crates/cs_formats`, `crates/cs_app`: **not touched**. This stage adds no BM
  layout or composition behavior; it reads a ROF member the F09 chain did not
  read before and joins it with the F09-PALETTE extraction.

**One observable failure:** before this stage no production code read the
paint-shop controls at all, and the paint shop's colour, shade and pattern-name
values were not even known to be absent — a future edit could have introduced a
swatch table with no test objecting. With this stage,
`accept_f09_paintshop_retail_option_space_and_recorded_gaps` fails on the first
control whose declared entry count, record line, field census or span does not
match the original member, `accept_f09_paintshop_reports_a_control_that_stores_a_value`
fails the moment any control record spells a colour literal, and
`PaintShopValue` has no variant that could carry a swatch colour at all.

## What the production extractor reads

`PaintShopCatalog::discover` reads one member and joins it with the F09-PALETTE
catalog:

| Property | Value |
| --- | --- |
| Container | `GOSDATA/ASSETS/crimson.rof` (`acc9946874110e9741183384010bc02fd48923ae3d608fcf02a96498c3731174`, 60 236 221 bytes) |
| Member | `ASSETS/LAYOUT.CSV` |
| Section | `[@Paint@]` (the reader reports the section name as `@Paint@`, the bytes between the brackets) |
| Stored extent | offset 37 359, length 15 078, SHA-256 `bad130fc40708a3dca98bc2628b81ac3218dbd9ead6d6b886f62cb34841a181` |
| Decoded extent | offset 37 359, length 56 148, SHA-256 `b50ea48bbe97ea098d76575115629bfbe6d0f55165fa364c939f9fb089892583` |
| Trailing bytes | 0 |

The member is stored compressed, so its stored and decoded extents differ and
**both** spans are recorded: `layout_span()` is the resolution's span (the stored
extent the container physically holds) and `decoded_span()` is the span the
keyed-list document was read against, with the digest of exactly the decoded
bytes. Neither is derived from the other; the production reader decides where a
line inside the member comes from, so a control's provenance is its
**container-absolute decoded span plus its 1-based line in the member**.

## The declared option space

Ten `D` (dropdown) records, each 12 fields, with the entry count the layout
declares and how the record spells itself. `colour` counts fields whose bytes
spell an eight-hex-digit `0x…` colour literal; `hex` counts other `0x…` values;
`text` is everything else, and the record letter is one of them.

| Record | slot | entries | line | placeholders | integers | colour | hex |
| --- | --- | --- | --- | --- | --- | --- | --- |
| `PT_D_PATTERN` | — | 12 | 826 | 8 | 3 | 0 | 0 |
| `PT_D_COLORS0` | 0 | 18 | 827 | 7 | 4 | 0 | 0 |
| `PT_D_COLORS1` | 1 | 18 | 828 | 7 | 4 | 0 | 0 |
| `PT_D_COLORS2` | 2 | 18 | 829 | 7 | 4 | 0 | 0 |
| `PT_D_SHADES0` | 0 | 10 | 830 | 7 | 4 | 0 | 0 |
| `PT_D_SHADES1` | 1 | 10 | 831 | 7 | 4 | 0 | 0 |
| `PT_D_SHADES2` | 2 | 10 | 832 | 7 | 4 | 0 | 0 |
| `PT_D_DECALS0` | 0 | 2 | 833 | 5 | 6 | 0 | 0 |
| `PT_D_DECALS1` | 1 | 2 | 834 | 5 | 6 | 0 | 0 |
| `PT_D_DECALS2` | 2 | 2 | 835 | 5 | 6 | 0 | 0 |
| **total** | | | | **65** | **45** | **0** | **0** |

The placeholders are the slider, its two arrows, the two drop arrows, `x`, the
item width and the item height (the pattern record spells eight of the twelve
positions as references, the nine per-slot lists spell seven or five); the
integers are `y`, `z`, the item geometry and the entry count. **No record spells
a colour, a hex value or a display name.** That census is the whole of the
engine-internal claim inside the readable source, and it is *derived*, not
asserted: a control that does spell a value stops its gap and the cross-check
reports it (`control_carries_value`).

The decal pane record `PT_P_DECALS` (line 802) declares
`P,PX_P_Decals.tga,0,0,0,50,0,2,1`: the art member and **50 frames**. Measured
separately (private, read-only, not committed): the sheet is a 66 × 3300
24-bpp TGA whose cells are 66 px tall in a single column, so the 50 frames tile
the image exactly, and columns 0 and 65 are pure background on all 3 300 rows.

The role names come from the original record-name stems themselves
(`PAINT_SHOP_STEMS`: `PT_D_PATTERN`, `PT_D_COLORS`, `PT_D_SHADES`,
`PT_D_DECALS`); the per-slot controls append their slot as the last character of
the key, which is how `PaintShopCatalog::control` finds the three lists. A stem
the original data does not spell has no role.

## The paint shop's own script (measured, not production code)

`ASSETS/SCRIPTS/PAINT.SCRIPT` (7 247 decoded bytes, member of `crimson.rof`,
stored extent offset 57 666 555, stored length 2 307, SHA-256 of the stored
extent `04d952369cc8e72f86d38e46af2155477a655f33d4e32d7a272dd54c1c7e5429`) is
plain CRLF text and names the native callbacks the shop uses. Offsets below are
**decoded-member-relative**; the member is stored compressed, so they are not
container-absolute.

| line | decoded byte | text |
| --- | --- | --- |
| 53 / 54 | 918 / 933 | `KLA.TJ = 2235` then `KLA.YC = "pt_d_pattern"` |
| 64 / 65 | 1 131 / 1 149 | `LLA[R].TJ = 2229` then `LLA[R].YC = "pt_d_colors" conv$(R)` |
| 77 / 80 / 83 | 1 323 / 1 357 / 1 391 | `MLA[0].TJ = 2232`, `MLA[1].TJ = 2233`, `MLA[2].TJ = 2234` |
| 86 | 1 419 | `MLA[R].YC = "pt_d_shades" conv$(R)` |
| 96 / 97 | 1 579 / 1 597 | `NLA[R].TJ = 2240` then `NLA[R].YC = "pt_d_decals" conv$(R)` |
| 126 / 128 | 2 147 / 2 209 | `callback($$E$$, 2237, 0, (R), LLA[R].QG)` and `callback($$E$$, 2236, 0, (R), MLA[R].QG)` |
| 160 | 2 681 | `callback($$E$$, 2238, 1, (sender.QG))` — the pattern selection |
| 169 / 178 | 2 802 / 2 992 | `callback($$E$$, 2237, 1, …)` and `callback($$E$$, 2236, 1, …)` — the colour and shade selections |
| 186 | 3 129 | `callback($$E$$, 2239, 1, (R), (sender.QG), …)` — the decal selection |
| 344 / 418 | 5 259 / 6 383 | `VLA = parent.QG * 5 + parent.BN.IN.YLA` and the matching `blit @paint@CLA.ZH (0, XB * (AMA * 5 + BMA) …)` |

Two things follow, and both matter:

* **The counts this stage exposes are the layout's.** `RecordSchema::Dropdown`
  names the twelfth field `TotalDisplayed`, and the dropdown list objects fill
  themselves through `callback($$E$$, (parent.TJ), BA, SLA[BA])` over
  `parent.UJ` items — an item count the engine keeps. The engine's own list
  lengths are not observable from any file, so this stage reports the layout's
  declared counts under that name and does not claim they equal the engine's.
  Where a second source does pin a count, the cross-check uses it: the pattern
  list declares 12 and `vehicle.zrd` names 12 paint patterns, and that agreement
  is asserted rather than assumed.
* **`5` is a UI addressing rule, not the decal id space.** The decal list draws
  five sheet frames per dropdown row (`row * 5 + 0..4`), so with two rows per slot
  the shop lists frames `0..9` of the 50 the pane declares, and the preview
  blits `selected row * 5 + decal id` where the decal id is the engine's. The
  vehicle records store indices `2..=21`. The two numbering schemes are not the
  same and neither readable member states the mapping.

This stage does **not** parse `PAINT.SCRIPT`. The `UiScript` dialect's reader is
`DialectReader::Deferred { stage: "F13-A" }` and its recorded unknowns are "the
whole grammar and every callback id"; writing a script reader here would take
F13-A's slice. The table above is research output, cited as such.

## Why the values are engine-internal (the negative search)

The claim is that no readable member of the installation stores a swatch colour,
a shade or a paint-pattern display name. Three independent searches, all
read-only and reproducible, all recorded here rather than in a script:

1. **Every member of every ROF container.** `crimson.rof` (846 members, 96 063 812
   decoded bytes) and `crimptch.rof` (1 member) were decoded with a private
   reader cross-checked against the production `cs-inspect rof` member list
   (846/846 spellings, offsets, stored lengths and compression bits agree, and
   every stored-extent digest matches the production digest). Case-insensitively,
   `paint_pattern`, `paint_color1` and `paint_decal1` occur in **zero** members.
   `pt_p_decals` and `pt_d_colors` occur in exactly two: `ASSETS/LAYOUT.CSV` and
   `ASSETS/SCRIPTS/PAINT.SCRIPT`.
2. **Every member of every version-one ZBD archive.** 64 containers, 6 334
   members (the `zbd-audit` inventory's reader and sound families). The byte
   strings `paint_pattern`, `paint_color1` and `paint_decal` occur in exactly one
   member: `ZBD/zrdr.zbd:vehicle.zrd`. The same search over the per-chapter
   archives finds the eight `ia.zrd` members this stage also reads (below) and
   nothing else that carries paint fields. `player_fortune`, `sactrust` and
   `blckswan` occur in `vehicle.zrd` only; the other pattern names also occur in
   briefing, AI, sound and scenario members, none of which carries a paint
   record. **`itstaxi` occurs in no readable member at all**; `broadway` occurs
   once, as the `ace_pattern` of `ZBD/C5/IA1/zrdr.zrd:ia.zrd`.
   (The search is a byte-substring search, so a short name can coincide inside a
   binary member — `cccp`, for instance, matches pixels in `.BM` and `.TIF`
   members. That is why the finding leans on the long `paint_*` field names and on
   parsing every member it names, never on a short name's absence.)
3. **The engine image's readable sections.** `crimson.icd`
   (`0e3b4724f045e0bedf7203cd40cdeb5b6e0b9a0bab78c3d04c278cb146e9833b`,
   2 580 578 bytes) is a PE32 whose `.text` (2 105 344 raw bytes, 7.91 bits/byte,
   5.71–7.96 per 64 KiB block) and `.data` (172 032 raw bytes, 7.99 bits/byte,
   7.97–8.00 per block) are encrypted, as F12-J recorded. `.rdata` (5.08),
   `.idata` (5.88) and `.rsrc` (3.51) are intact. The 22 non-degenerate RGB
   triples `vehicle.zrd` stores were searched in `.rdata`, `.idata` and `.rsrc`,
   in both channel orders: **zero hits**. (The only triples that do occur are
   `(255,255,255)` and `(0,0,0)`, which are degenerate.) No 18-entry or 10-entry
   palette-shaped table was found in the intact sections either.

So the shop exists in readable form only as its `[@Paint@]` controls, and those
controls have no field in which a value could be written. The values are
engine-internal, and the implementation's shape follows: `PaintShopValue` has no
variant that carries one.

## What this closes, and what it does not

Resolved against the five unknowns F09-PALETTE recorded
(`docs/findings/evidence/F09-PALETTE.json`):

1. **`player_palette`** — narrowed, still open. `player_fortune` is named by the
   `devastator` and `wingman` records and stores no triple, and the shop's
   pattern list has exactly one entry per paint pattern the vehicle records name
   (12 = 12), so the player's paint is a **shop choice** rather than a stored
   scheme. Its starting swatches and shades are the shop's engine-internal
   values. Affected content: the player paint on every airframe. Resolving task:
   #358 (owner run of the paint shop) or #351 (owner-authorised engine-image
   unpack).
2. **`missing_faction_palettes` (BROADWAY, ITSTAXI)** — resolved as *not paint
   patterns*. The shop's pattern list is exactly the twelve names `vehicle.zrd`
   stores and neither of these two is among them, and neither string occurs in any
   readable member, so the paint shop does not offer them and cannot recolour
   them. Both have stock BM livery directories (F09-D), so their appearance comes
   from the shipped BM planes themselves. Which stock livery directory
   corresponds to which paint pattern stays engine-internal: the directories are
   `BLACKHAT`…`STUDIO` plus `BROADWAY`, `FORTUNE` and `ITSTAXI`, while the player's
   pattern is named `player_fortune`, so the naming is not a mapping. Affected
   content: those two factions' liveries and the directory-to-pattern binding.
   Resolving task: #358 or #351, with F17-D for the renderer.
3. **`decal_index_semantics`** — narrowed, still open. The decal pane declares a
   50-frame sheet and **every** stored `paint_decal` index (`2..=21`) is inside it,
   so the sheet bounds the id space; the cross-check reports a stored index at or
   past the declared frame count as a `decal_outside_sheet` finding rather than
   dropping it. The mapping from a stored index to a frame, and the shop's own
   two-entries-per-slot list, are not established by any readable member.
   Affected content: every decal selection. Resolving task: #358 with F17-D.
4. **`shade_multipliers`** — proven engine-internal. The shade dropdowns declare
   10 entries each and store no value. Affected content: every shaded paint
   variant. Resolving task: #358 or #351.
5. **`slot_to_mask_mapping`** — untouched; it belongs to F17-D (composition
   agreement), not to this stage. The shop gives one dropdown per mask plane
   (three slots), which is consistent with but not evidence for the mask mapping.

Also not claimed: a `verified_original` or `release_approved` level, anything about
how the original renderer bound a composed texture, and anything about the
original game running. No original run was observed in this session.

## Tests

Five `accept_f09_paintshop_` tests, all in `crates/cs_content/src/livery.rs`,
plus one ignored evidence harness beside them. Every assertion is produced by
production code; each fails if the behavior it pins is removed. Four run on an
ordinary build (CI); the retail test and the harness are
`#[ignore = "requires CS_GAME_DIR"]` and fail loudly when run without it.

| Test | What it pins |
| --- | --- |
| `accept_f09_paintshop_reads_the_declared_option_space_from_the_layout` | the ten controls with their declared counts and roles, every control's line checked against the member it was read from, the summed field census, the decal pane's art and frame count, the four gaps with their counts and censuses, `EngineInternal` for every in-range swatch/shade/decal/pattern query, `NotOffered` past the declared count and `NoControl` for a slot the shop does not declare |
| `accept_f09_paintshop_refuses_a_layout_or_control_it_cannot_read` | a layout with no `[@Paint@]` section (`missing_section`), a control spelled as a pane (`wrong_kind`), a control one field short (`short_record`), an entry count spelled as a `<NAME>` (`entries_unreadable`), a slot digit above the shop's slots (`unknown_slot`) and an unreadable frame count (`frames_unreadable`) |
| `accept_f09_paintshop_reports_a_control_that_stores_a_value` | a control whose `x` position spells a colour literal: the census measures `colour: 1`, the **swatch gap is not recorded** (a gap that survived a control that stores its values would be a lie) and the other gaps are unaffected |
| `accept_f09_paintshop_cross_check_closes_patterns_and_bounds_decals` | a pattern list that matches the stored pattern count reports only the uncoloured pattern; a list of three against two stored patterns reports `pattern_count_mismatch`; a stored decal inside the declared frames is not a finding and one past it is `decal_outside_sheet`; a layout with no decal pane reports `no_decal_sheet` |
| `accept_f09_paintshop_retail_option_space_and_recorded_gaps` | the original member: the stored and decoded spans and digests, zero trailing bytes, all ten controls with their counts, lines and per-record censuses, the summed census with `colour: 0` and `hex: 0`, the decal pane's art, 50 frames and line, exactly one cross-check finding (`pattern_without_palette`), and the 12 = 12 closure between the pattern list and the stored patterns |
| `evidence_report_f09_paintshop_writes_the_acceptance_report` (ignored harness) | writes the acceptance report from the recorded suite, the production discovery/fingerprint and the production catalog plus its cross-check; it is **not** named `accept_f09_paintshop_*` so the task selection cannot pick it up |

Mutation checks, applied to the production code and reverted, with the tests that
caught each:

- dropping the `colour`/`hex` guard in `paint_shop_gaps` → the value-carrying
  fixture would still record a swatch gap; `…reports_a_control_that_stores_a_value`
  fails;
- reading the entry count from position 10 instead of 11, or accepting any field
  as a whole number → the retail test fails on the control's declared count and
  `…refuses_a_layout_or_control_it_cannot_read` fails on the placeholder case;
- accepting any record under a paint-shop stem without the record-kind check →
  the pane fixture reads as a dropdown; `…refuses_…` fails on `wrong_kind`;
- comparing the section name against `[@Paint@]` instead of the reader's
  `@Paint@` → every test fails with `missing_section` (this is what the first run
  did; the finding records it rather than hiding it);
- replacing the slot parse with a `strip_suffix` → the per-slot controls lose
  their slot; the control table fails.

## Evidence

This task used `retail` (read-only access to `$CS_GAME_DIR`), so
`docs/contracts/CLI-EVIDENCE.md` requires an acceptance report. It is
`docs/findings/evidence/F09-PAINTSHOP.json`, written by the inline harness from a
real run: the recorded `cargo test --workspace --locked -- accept_f09_paintshop_
--include-ignored` log (5 tests, all pass), the production `install::discover` /
`fingerprint` / `content_fingerprint` of the installation, the production
`PaintShopCatalog::discover` over the original member, the production
`FactionPaletteCatalog::discover` over `ZBD/zrdr.zbd` member `vehicle.zrd`, their
cross-check, and the private `paint-shop-catalog.json` artifact (per-control
counts, lines, field censuses, both spans, the decal sheet declaration, the four
gaps, the one finding and the `option_space_fingerprint`
`ca8d21fcf1ce3383e887cc40663c973e33a4cb6a05d901e22e114355cb5c2940`). No original
bytes, text or images are committed. The report's `capabilities` are
`["retail", "synthetic"]`; `claim` is `implemented` — a merge awards `checked` at
most and nothing here observed the original game running.

The report is validated with `tools/validate_evidence.py` **without**
`--require-pass`, for the reason the F09-PALETTE, F12-G and F12-H reports give:
the flag rejects a report that lists unresolved issues, and this task's
deliverable is that six limitations stay recorded. Concretely, on this tree:

| Command | Result |
| --- | --- |
| `python3 tools/validate_evidence.py private/evidence/F09-PAINTSHOP/acceptance.json --artifact-root private/evidence/F09-PAINTSHOP` | 0 (`structurally_valid: true`, 2 artifacts) |
| `python3 tools/validate_evidence.py … --require-pass` | **3** (`Unresolved issues`) — expected: the six recorded limitations are the deliverable |

## Boundaries

This stage reads the paint shop's declared option space from the original layout
and records which of its values no readable member stores. It does not claim the
swatch palette, the shade table, the pattern display names, the decal-index
mapping or the player's default paint; those are the six recorded limitations
above, each naming its affected content and what resolves it. A
`verified_original` claim about paint-shop appearance still needs an owner
capture or an owner-authorised engine-image unpack and is not made here. Nothing
here observes how the original renderer bound a composed texture (F17-D), and the
slot-to-mask mapping stays F17-D's.