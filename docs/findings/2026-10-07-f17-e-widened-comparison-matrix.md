# F17-E: the comparison matrix over every world group, with textured captures

Date: 2026-10-07. Task: F17-E-MATRIX-COVERAGE (Rally #736) — *"Widen the F17-D
comparison matrix to every world group and to textured captures"*, extending
`specs/F17-rendering-material-fidelity-and-scalable-presentation.md` AC04.
Shared contract: `docs/contracts/CLI-EVIDENCE.md`. Capabilities used:
**`retail`** (read access to the owner's installation at `$CS_GAME_DIR`) and
**`gpu`** (14 frames drawn on the real adapter, Apple M3 Pro / Metal). No
`audio`, `human_play` or `human_review` was used or available: nothing here is
`verified_original`, and no original screenshot exists to compare against —
that side of the comparison stays with `#358 REF-OWNER-FIRST-CAPTURE`.

## Files

- `crates/cs_app/src/render/matrix.rs`: `resolve_wide` +
  `WorldGroupSource`/`GroupMatrix`/`WidenedMatrix` over every offered group,
  and `MatrixError::ContainerRefused` for a container that would not read.
- `crates/cs_app/src/playtest_retail.rs`: `read_all_playtest_sources` — one
  `install::discover`, the shared `planes.zbd`, then each discovered group's
  `gamez.zbd` and group-local texture archive as `Result`s on the group's
  entry.
- `crates/cs_app/src/world/textured_capture.rs` (new): the textured GPU
  capture — `WorldMeshes::insert_render_mesh` for geometry, the group's own
  archive through `TextureBinder` for materials, `gpu_capture`'s shared
  framing/readback constants, `ComparisonSettings::comparison()` recorded on
  the result.
- `crates/cs_app/src/world/gpu_capture.rs`: constants and helpers raised to
  `pub(super)` so the textured sibling shares rather than copies them;
  `drive_capture` takes the PNG path explicitly.
- `crates/cs_app/src/world/mod.rs` (wiring only): `pub mod textured_capture;`
  plus its re-exports.
- `crates/cs_app/tests/render/matrix_wide.rs` (new): seven `accept_f17_e_*`
  tests — five fast, two `#[ignore]`d (`retail`, `retail`+`gpu`).
- `crates/cs_app/tests/render/evidence.rs`: `evidence_report_f17_e`, plus
  shared `parse_suite`/`capture_artifacts`/`rows_json` taking prefixes.
- `docs/findings/evidence/F17-E-MATRIX-COVERAGE.json` and this file.

**One observable failure.** Before this stage the matrix was pinned to
`MATRIX_WORLD_GROUP` (`C1C`) and the capture drew only the flat geometry
witness. The narrowing that fails the suite: if `resolve_wide` collapsed back
to one group,
`accept_f17_e_every_offered_group_is_a_matrix_entry_in_offered_order` fails on
the group count; if `part_outcomes` stopped flagging an unbound texture,
`accept_f17_e_a_missing_texture_is_a_refusal_never_a_fallback` fails on both
the binder-recorded and the silent case.

## What the retail run measured

`private/evidence/F17-E-MATRIX-COVERAGE/widened-matrix.json` (derived counts
and names only) records the eight discovered groups, each a five-row matrix:

| group | skyline | vegetation | night_effects | archive |
| --- | --- | --- | --- | --- |
| `C1` | `h_zone1scroll` (98 tri) | **refused**: two families repeat (`g27816`, `g0`) | `moon` (2 tri) | opened |
| `C1B` | `g1163` (52 tri) | **refused**: two families repeat (`g27816`, `g0`) | **refused**: no `moon`/`stars` node | opened |
| `C1C` | `h_zone2scroll` (52 tri) | `g27817` (2 tri) | `moon` (2 tri) | opened |
| `C2` | `h_zone2scroll` (72 tri) | `g27817` (2 tri) | **refused**: no `moon`/`stars` node | opened |
| `C2B` | `g1167` (52 tri) | `g27817` (2 tri) | **refused**: no `moon`/`stars` node | opened |
| `C3` | `g1155` (74 tri) | **refused**: no repeated child name | **refused**: no `moon`/`stars` node | opened |
| `C4` | `g1168` (82 tri) | `o717` (2 tri) | **refused**: no `moon`/`stars` node | opened |
| `C5` | `g1171` (138 tri) | **refused**: seven family names repeat | `moon` (2 tri) | opened |

The shared `zbd/planes.zbd` resolves both aircraft rows identically in every
group (`g366`, 231 tri / `g442`, 251 tri). `C1C` — the F17-D pinned group —
is the only group where all five subjects resolve; every other group carries
at least one **named** refusal, which is the required shape: a gap is a row
with a reason, never a missing entry.

Measured anchor facts worth recording: the instanced-family rule is
*ambiguous* on `C1`/`C1B` (`g27816` and `g0` both repeat) and on `C5` (seven
repeated names), and *absent* on `C3` (no repeated child name at all); the
`moon|stars` night-sky nodes exist only in `C1`, `C1C` and `C5`; every group
stores a `horizon` child with a drawable mesh.

## The textured captures

`capture_subject_textured` drew every resolved world-side subject through its
own group's archive — 14 frames:

| capture | textured parts | images bound | coverage |
| --- | --- | --- | --- |
| `c1 skyline` | 1/2 | `sky2` | 169‰ |
| `c1 night_effects` | 1/1 | `moon1` | 192‰ |
| `c1b skyline` | 1/1 | `sky1` | 111‰ |
| `c1c skyline` | 1/1 | `sky1` | 111‰ |
| `c1c vegetation` | 1/1 | `cloud1` | 86‰ |
| `c1c night_effects` | 1/1 | `moon1` | 192‰ |
| `c2 skyline` | 1/1 | `sky1` | 110‰ |
| `c2 vegetation` | 1/1 | `cloud1` | 86‰ |
| `c2b skyline` | 1/1 | `sky1` | 111‰ |
| `c2b vegetation` | 1/1 | `cloud1` | 86‰ |
| `c4 skyline` | 1/1 | `c4sky2` | 111‰ |
| `c4 vegetation` | 1/1 | `cloud1` | 83‰ |
| `c5 skyline` | 4/4 | `c5sky2`, `star1` | 186‰ |
| `c5 night_effects` | 1/1 | `moon1` | 192‰ |

The bound names differ per group (`sky1`, `sky2`, `c4sky2`, `c5sky2`,
`cloud1`, `moon1`, `star1`) — evidence the group-local archive, not a shared
one, is doing the binding.

**The `missing_texture` refusal was exercised for real**: `C3`'s skyline mesh
(492) has two material groups naming `cloud1.tif` and `cloud2.tif`, which
`ZBD/C3`'s archive does not resolve (`texture_not_resolved`) — the capture
refused by name and left no PNG, where the general playtest path would have
drawn a neutral stand-in.

## Design decisions worth a second look

- **The binder is #666's `TextureBinder`, not F17-B's `upload_image`.** Every
  retail decoder leaves `DecodedImage::color_space` at `ColorSpace::Unknown`,
  which the strict F17-B adapter refuses — so `upload_image` cannot bind any
  retail texture today. The textured capture therefore uses the production
  playtest path (`PlaytestTextureArchive` + `TextureBinder`, provisional
  presentation: sRGB, repeat addressing, keyed coverage as a 0.5 mask) and
  records that claim on the result. The task's *"(F17-B `upload_image`, #666
  `playtest_textures`)"* parenthetical names the upload path's two halves;
  choosing the part that actually reaches retail bytes is the honest reading.
- **`missing_texture` is layered on top of the binder, not inside it.** The
  #666 binder keeps its `Binding::Neutral` fallback (correct for the
  playtest); `part_outcomes` re-classifies each drawn part against the
  material table and the binder's `unresolved` list, and the capture refuses
  before a frame is drawn when a named texture is unbound — including the
  `texture_not_bound` case where the group stores no UV to sample it with.
- **One discovery, all groups.** `install::discover` hashes the whole
  installation (~4½ min on this machine); `read_all_playtest_sources` pays it
  once and keeps each group's `container`/`textures` outcome as a `Result`,
  which is what makes "never silently skipped" structural.

## What is not claimed

- The bound presentation is the **#666 designed reading**, declared
  provisional (`PLAYTEST_TEXTURE_PRESENTATION_IS_PROVISIONAL`): these frames
  prove the group's own textures resolve through the production path and draw
  on the subject's real geometry — they are not the original renderer's
  output, and `c1c vegetation` drawing `cloud1` is a name-reading result to
  hold lightly, not a finding that billboards are clouds.
- No original-run capture exists; nothing here is `verified_original`.
- `claim` in the report is `implemented`; the merge awards `checked`.
