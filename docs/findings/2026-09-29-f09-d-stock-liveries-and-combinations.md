# F09-D: Known stock liveries and every discovered valid combination

Date: 2026-09-29. Task: F09-D "Verify known stock liveries and every discovered
valid combination" (`specs/F09-bm-multilayer-liveries-and-paint-composition.md`,
section `### F09-D`; AC04 and non-negotiable #4). Shared contract:
`docs/contracts/IDENTITY-CONTENT.md`. Test prefix: `accept_f09_d_`.
Capabilities used: `retail` (read-only `$CS_GAME_DIR`) plus ordinary build/test.
Evidence: `docs/findings/evidence/F09-D.json`; private artifacts stay in
`private/evidence/F09-D/`.

## Files and the one observable failure (listed before editing)

- `crates/cs_content/src/livery.rs`: the content-layer production path for this
  stage — `LIVERY_GRAPHICS_DIRECTORY`, `LiveryName`, `parse_livery_spelling`,
  `StockLivery`, `LiveryFinding`, `LiveryCombination`, `StockLiveryCatalog`
  (`new`, `add`, `discover`, `assets`, `findings`, `factions`, `prefixes`,
  `combinations`), the six inline `accept_f09_d_*` tests and the inline ignored
  evidence harness.
- `crates/cs_formats/src/bm.rs`, `crates/cs_formats/tests/bm.rs`,
  `crates/cs_app/src/livery.rs`: **not touched**. This stage verifies the
  existing F09-A/B/C reader, composition and store; it adds no layout or
  composition behavior, so the format crate, its tests and the app layer are
  unchanged.
- This file and `docs/findings/evidence/F09-D.json`.

**One observable failure:** before this stage the production code could read and
compose a BM but never said which the installation actually ships or which
faction/airframe combinations exist. A catalog that drops a member it cannot
parse, or that groups the wrong prefix, would pass every earlier test. With this
stage, `accept_f09_d_retail_stock_liveries_and_combinations` measures the real
library: a member that is not the observed subset, a prefix/faction count that
changes, or a member read from the wrong layout fails, and
`accept_f09_d_retail_composed_liveries_match_the_pinned_reference` fails on the
first texel that does not match the pinned reference.

## Sources

- S09, pinned blob `ec196de05f532cc3c286ccf8fead363bf76e5c63`
  (`extract_bm.py`): the observed BM subset — little-endian `u16` height then
  `u16` width, RGB base (3N), three `u8` masks (N each) and an RGBA overlay (4N),
  `4 + 10N` bytes, each plane exported vertically flipped.
- S10, pinned blob `ccf7c4ea065c17a44d354e8d703561cdc94dd518`
  (`set_paintjob.py`): the airframe prefix list, the `FACTION_COLORS` table and
  `apply_color_mask` + `Image.alpha_composite` composition. The blob also names
  the stock-livery directory layout this stage parses.
- `docs/research/FORMAT-NOTES.md`, "BM observed subset [S09, S10]".
- F09-A/B/C findings:
  `docs/findings/2026-09-28-f09-a-bm-layout-and-rectangular-fixture.md`,
  `docs/findings/2026-09-28-f09-b-deterministic-layered-composition.md`,
  `docs/findings/2026-09-29-f09-c-model-instances-and-construction-preview.md`.
  F09-B/C call the exact weights, rounding and alpha convention *observed tool*
  and leave retail agreement to this stage.
- The pinned reference generator, `private/f09d-ref/generate_reference.py`
  (private, never committed): Pillow 12.3.0, installed privately under
  `private/f09d-py`. It decodes each member with the S09 plane walk and composes
  it with the S10 algorithm and S10's `FACTION_COLORS`, writing a raw RGB8 image
  (8-byte little-endian width/height header) per member and paint plus a
  `manifest.txt`.

## The pinned reference

The reference is a **tool**, not the original renderer. No stage of this project
has a GPU consumer for a composed `ComposedLivery` (F17-B is later), so
AC04's "from several angles with the original" cannot be taken literally with the
capabilities this project currently has; what is compared here is the composed
texture at every texel against an independent implementation of the pinned tool
algorithm:

1. The 184 `.bm` members of `GOSDATA/ASSETS/crimson.rof` were decoded into
   `private/f09d-members/` through the production F04 ROF reader by a temporary
   probe that was removed after the dump (no probe code remains in the tree).
2. `generate_reference.py` reads each member's S09 planes, composes stock paint
   (S10 `FACTION_COLORS` for the member's own faction) and one private custom
   paint (`#C81E28`, `#0ADC5A`, `#FAFAFA`) with the S10 algorithm, and writes
   `ASSETS_GRAPHICS_<FACTION>_<NAME>.stock.rgb` /
   `.custom.rgb` plus `manifest.txt`.
3. The `accept_f09_d_retail_composed_liveries_match_the_pinned_reference` test
   composes the same member twice through the production `compose_livery` and
   requires **zero** differing texels against each reference image. It also
   reports how many texels a vertical flip would differ in, so an orientation
   error is named rather than hidden.

The reference colours are S10's research leads: they are the reference's input,
never a production palette (non-negotiable #4 keeps a palette out of
`cs_content`/`cs_app`). The comparison therefore establishes agreement between
two readers of the same observed algorithm — Pillow and our Rust — not agreement
with the original game.

The private reference directory is overridable with `CS_F09_D_REFERENCE`; the
default is `private/f09d-reference`.

## The measured retail library

`StockLiveryCatalog::discover` walks `GOSDATA/ASSETS/crimson.rof` through the
production ROF reader and verifies every `.bm` member:

| | |
| --- | --- |
| container | `GOSDATA/ASSETS/crimson.rof` (846 members) |
| `.bm` members verified | **184**, all inside the observed subset |
| findings | **0** (no unparsed, unreadable or off-layout `.bm` member) |
| factions | 14 |
| airframe prefixes | 11 |
| prefix x faction combinations | **33** |

Every member is exactly `4 + 10*w*h` bytes (zero uncovered tail), with dimensions
from `32x32` up to `256x128`. The per-prefix table (each cell is the number of
`<PREFIX>_*` assets that faction stores):

| Prefix | Faction: assets |
| --- | --- |
| `AGYRO` | `BLACKHAT`: 4, `FORTUNE`: 4, `ITSTAXI`: 4, `STUDIO`: 4 |
| `BAL` | `BRITISH`: 5, `FORTUNE`: 5 |
| `BLO` | `BLAKE`: 6, `FORTUNE`: 6, `HUGHES`: 6 |
| `BRI` | `BLACKHAT`: 9, `FORTUNE`: 9, `MEDUSAS`: 9 |
| `DEV` | `CCCP`: 6, `FORTUNE`: 6 |
| `FIR` | `FORTUNE`: 6, `HOLLYWD`: 6 |
| `FUR` | `BLCKSWAN`: 5, `FORTUNE`: 5, `HUGHES`: 5, `STUDIO`: 5 |
| `HEL` | `FORTUNE`: 4, `GERMAN`: 4, `SACTRUST`: 4 |
| `KES` | `FORTUNE`: 6, `HUGHES`: 6, `MEDUSAS`: 6 |
| `PEA` | `BLAKE`: 6, `BRITISH`: 6, `BROADWAY`: 6, `FORTUNE`: 6 |
| `WAR` | `BLACKHAT`: 5, `FORTUNE`: 5, `SACTRUST`: 5 |

The 14 factions are `BLACKHAT`, `BLAKE`, `BLCKSWAN`, `BRITISH`, `BROADWAY`,
`CCCP`, `FORTUNE`, `GERMAN`, `HOLLYWD`, `HUGHES`, `ITSTAXI`, `MEDUSAS`,
`SACTRUST`, `STUDIO`. The combination table, the per-member dimensions and a
per-member digest of the parsed planes are in the private
`private/evidence/F09-D/livery-catalog.json`; the single catalog fingerprint
over all 184 spellings and plane digests is
`fa036511d7d627959764fce09fe8589b32aca5a03276f13c041f120df4a01296`.

Corpus endpoint coverage (from the production `BmFile` accessors over all 184
members): mask bytes `0 = 6,092,775`, `255 = 2,786,275`, in between
`= 318,518`; overlay alpha `0 = 1,805,690`, `255 = 18,328`, in between
`= 1,241,838`. Both endpoints and the interior are therefore exercised by the
real data.

## Tests

| Test | Covers |
| --- | --- |
| `accept_f09_d_spelling_names_faction_prefix_and_part` | a `GRAPHICS/<FACTION>/<PREFIX>_<PART>.bm` spelling parses, extension case-insensitively; a non-`GRAPHICS` path, a missing part/prefix and non-`.bm` files are refused |
| `accept_f09_d_catalog_discovers_combinations_and_keeps_findings` | valid prefix x faction grouping; an off-layout `.bm`, a truncated BM and a non-BM member are kept as findings (codes) rather than dropped |
| `accept_f09_d_catalog_records_dimensions_tail_and_refusals` | dimensions, exact `4 + 10*w*h` covered length and a nonzero tail are recorded; a zero-sized header is an `empty_image` finding |
| `accept_f09_d_catalog_is_deterministic_in_spelling_order` | two enumeration orders give the same asset list and combination table |
| `accept_f09_d_retail_stock_liveries_and_combinations` (ignored without `CS_GAME_DIR`) | the whole retail result above: 184 assets, 14 factions, 11 prefixes, the pinned per-faction counts, zero findings, zero tails, endpoint coverage |
| `accept_f09_d_retail_composed_liveries_match_the_pinned_reference` (ignored without `CS_GAME_DIR`) | every texel of 184 members x (stock paint + one custom paint) matches the pinned reference, decal overlay alpha included; the catalog and the reference name the same members |

Six tests, all calling production code: the synthetic ones call
`parse_livery_spelling` / `StockLiveryCatalog`; the retail ones call
`StockLiveryCatalog::discover` and `compose_livery` over the real library. CI
skips the two retail tests (no original data) and runs the four synthetic ones;
without `CS_GAME_DIR` the retail tests panic rather than pass.

## Mutation probes

Each mutation was applied on this branch, the `accept_f09_d_` tests run and the
file restored; no probe text remains in the tree.

| Mutation | Failing tests |
| --- | --- |
| `multiply` rounds instead of truncating | `accept_f09_d_retail_composed_liveries_match_the_pinned_reference` |
| `alpha_over_opaque` always returns the destination | `accept_f09_d_retail_composed_liveries_match_the_pinned_reference` |
| `BM_STORED_ROW_ORDER` flipped (`TopDown`) | `accept_f09_d_retail_composed_liveries_match_the_pinned_reference` |
| `parse_livery_spelling` drops the `GRAPHICS` directory check | `accept_f09_d_spelling_names_faction_prefix_and_part`, `accept_f09_d_catalog_discovers_combinations_and_keeps_findings` |
| `combinations` keeps only the first faction of each prefix | `accept_f09_d_catalog_discovers_combinations_and_keeps_findings` |
| an asset is filed under its prefix as its faction | `accept_f09_d_catalog_records_dimensions_tail_and_refusals`, `accept_f09_d_catalog_discovers_combinations_and_keeps_findings`, `accept_f09_d_retail_stock_liveries_and_combinations` |

The three composition mutations show that the retail reference comparison is
the only `accept_f09_d_` test that fails; the catalog mutations show the
synthetic tests fail independently of the retail data.

## Recorded unknowns

These are scope boundaries with resolving work, not issues with the
`implemented` claim this stage makes (the stock-livery inventory, the
faction/prefix sets and every prefix x faction combination are measured from the
original library itself). The report's `unknowns` array is therefore empty: the
validator reserves it for unresolved issues with the claim. The boundaries are
never dropped to satisfy that gate — each is named in the report's
`review.method` (the harness emits [`DEFERRED_BOUNDARIES`] verbatim), names the
affected content and the resolving Rally task, and is filed as its own Rally task
so it survives this task being marked done and gates the fidelity claims it
affects.

- **Retail composition agreement.** The reference is the pinned S09/S10
  *tool* algorithm run under Pillow, not the original renderer. The production
  composition matches it at every texel of the whole library, but whether the
  original game applies the same mask weights, rounding and overlay alpha is
  **not established**. Affected content: every composed livery. Resolving task:
  **F17-D** (needs the **F17-B** GPU consumer and an owner-run capture). It gates
  any `verified_original`/release claim about livery appearance.
- **Retail faction palette.** The reference paints with S10's `FACTION_COLORS`,
  a research lead, because no original-data palette has been extracted. Which
  colours a faction really uses is **unknown** (non-negotiable #4). Affected
  content: every faction's base/mask colors on every composed livery. Resolving
  task: **F09-PALETTE (Rally #385)**. It gates any palette or faction-color
  fidelity claim.
- **Prefix -> airframe.** The 11 prefixes are the original names' own, but which
  airframe each names comes from original data that was not read here
  (non-negotiable #3). Affected content: the airframe identity of every stock
  livery. Resolving task: **F09-PREFIX (Rally #386)**.
- **On-screen / several-angles comparison.** AC04's literal "from several angles
  with the original" needs a GPU consumer (F17-B) and `human_play`/owner capture,
  neither of which this stage has. Affected content: any visual fidelity claim
  about a painted aircraft; the texel-level comparison against the pinned tool
  reference is the only livery composition evidence this stage has. Resolving
  task: **F17-D** (consumer **F17-B**). It gates **F63-D** and any release or
  visual-fidelity approval.
