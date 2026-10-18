# F10-F.1: the GameZ polygon two-sided flag, read into the render mesh

Date: 2026-10-18. Task: #1406 (`F10-F.1`), follow-up of F10-D.2 (#396).
Owner paths used: `crates/cs_formats/src/gamez/`,
`crates/cs_content/src/mesh.rs`, `crates/cs_app/src/render/`, plus the
crates' own test locations (`crates/cs_formats/tests/gamez/`,
`crates/cs_app/tests/render/`), which the F10 and F17 sheets own.

**Claim class: static code evidence, never `verified_original`.** The bit's
meaning is measured by static analysis of the owner-supplied decrypted
executable, accepted by the owner in place of a capture (`OWNER-STATIC-2026-10-05`,
recorded in
`docs/findings/2026-10-14-f10-d-2-front-face-winding-and-culling.md`). No
original run produced anything here; the retail census below is a measurement
of the *stored* flag over the owner's installation, read by production code,
not a claim that the original executable was run.

## The one observable failure

F10-D.2 measured the culling *rule* — an outline polygon whose first three
stored corners run counter-clockwise is front, a polygon with the stored flag
bit `0x400` is drawn from both sides — but no production reader interpreted the
flag. `cs_content::mesh::RenderMesh` ignored `RawPolygon::raw_flags`,
`MaterialFacts::two_sided` stayed `None` for every stored surface, and
`cs_app::render::bevy_state::render_state` refused every real surface with
`StateError::CullFaceUnknown`. A surface whose two-sidedness the renderer
invents silently halves or doubles it. This task reads the stored flag into the
render pipeline so a stored two-sided outline reaches the renderer as
`CullingClass::TwoSided`, and records the one case the group model cannot
express.

## Where the fact is kept

`0x400` is the stored polygon dword's bit 10, i.e. bit 2 of the flag field
`RawPolygonInfo::flags` returns. It is **per polygon**, not per material, so the
render mesh — not the material record — is where it is preserved losslessly:

* `cs_formats::gamez::FLAG_TWO_SIDED` names the bit (the pinned reference calls
  the same bit `UNK2`; `FLAG_TWO_SIDED == FLAG_UNK2`), and
  `RawPolygon::is_two_sided()` / `RawPolygonInfo::is_two_sided()` report it.
* `RenderTriangle::two_sided` is the **drawn** two-sidedness of the stored
  polygon: an **outline** with the bit is two-sided; a strip is never, because
  the original draws strips `D3DCULL_CW` and never runs the per-polygon face
  test on them.
* `RenderGroup::two_sided: Option<bool>` states it for the group a single draw
  call indexes: `Some(true)` when every source polygon is two-sided,
  `Some(false)` when none is, and **`None` when the group mixes both** — the
  group is the granularity one cull face can express, so the mixed fact is
  recorded as unknown, never rounded to a side. The per-triangle fact still says
  which triangles are which, so nothing is lost.
* `cs_app::render::bevy_mesh::GroupUpload::two_sided()` carries the group's fact
  (or a normal-split part's own subset, computed the same way) to the renderer.
* `cs_app::render::bevy_state::render_state_for_group(material, stored)` uses it
  when the classification left the fact open. A material record alone still
  declares no two-sidedness, so `render_state` is unchanged and still refuses an
  undeclared surface; the stored group fact closes that gap.
  `capture::upload_surface` supplies it from the uploaded group.

## Strips are reported, not trusted

No retail strip carries the bit, and the original never applies the per-polygon
face test to a strip. A strip that stores `0x400` is therefore **reported**
(`cs_formats::gamez::ParseFinding::StripTwoSidedFlag`) and read as one-sided by
the render mesh, rather than trusted as two-sided.

## The retail census

`FaceCensus` counts the flag as a stored fact, independent of a face's draw
outcome: `two_sided_outline_faces` and `strip_two_sided_flag_faces`. Over the
nine archives production discovery names, read by the production reader:

| archive | two-sided outline polygons |
| --- | --- |
| `ZBD/planes.zbd` | 7 043 |
| `ZBD/C1/gamez.zbd` | 7 386 |
| `ZBD/C1B/gamez.zbd` | 4 074 |
| `ZBD/C1C/gamez.zbd` | 3 643 |
| `ZBD/C2/gamez.zbd` | 4 989 |
| `ZBD/C2B/gamez.zbd` | 3 361 |
| `ZBD/C3/gamez.zbd` | 6 126 |
| `ZBD/C4/gamez.zbd` | 7 819 |
| `ZBD/C5/gamez.zbd` | 8 180 |
| **total** | **52 621** |

The total is exactly the owner note's `OWNER-STATIC-2026-10-05` count for the
outline case, and **0 of the 22 124** strips carry the bit (the test asserts
both, per archive, and that the reader raised no strip finding).

## Tests

* `cs_formats`, `accept_f10_f_1_census_counts_two_sided_outlines_and_reports_strip_flags`:
  a stored outline with the bit is one two-sided outline, a strip with the bit
  is counted separately and never trusted.
  `accept_f10_b_gamez_flags_select_strip_and_normals` additionally pins that a
  strip storing the bit yields the `strip_two_sided_flag` finding, and that an
  outline's bit is `RawPolygon::is_two_sided()`.
* `cs_content`, `accept_f10_f_1_a_stored_two_sided_outline_reaches_its_render_group`,
  `…_a_group_mixing_one_and_two_sided_polygons_records_unknown`,
  `…_an_unflagged_outline_records_a_one_sided_group`,
  `…_a_strip_two_sided_flag_is_not_trusted_by_the_render_mesh`, and the
  `#[ignore]`d retail census
  `accept_f10_f_1_retail_two_sided_polygons_are_counted_per_archive`.
* `cs_app`, `accept_f10_f_1_a_stored_two_sided_polygon_reaches_the_culling_class`
  and `accept_f10_f_1_a_mixed_group_is_refused_not_rounded`, in
  `crates/cs_app/tests/render/winding.rs`: a stored-flag mesh is driven through
  the production adapter and state reader to `CullingClass::TwoSided` (cull
  face `None`) and to the one-sided `Some(Face::Back)` case, and a mixed group
  is refused with `two_sided_unknown` rather than rounded.

## Still open (not claimed here)

* **The back side's negated-normal lighting.** The original drew the back side
  of a `0x400` polygon with the face normal negated; only the *culling* half is
  reproduced. The lighting path stays unmeasured/unreplicated.
* **Billboard meshes** (word0 == 1, word1 ∈ {0, 1}) still do not produce
  `CullingClass::Billboard` — that is F10-F.2 (#1407).
* **Strip culling parity** after the F10-B reader triangulates strips is still
  not re-derived.
* **The edge-on / zero-normal elimination** (`|d| ≤ 0.00005`) is F17-F.3
  (#1408).

## Commands and sources

See the task hand-over for the exact commands and exit codes. Sources:
`OWNER-STATIC-2026-10-05`;
`docs/findings/2026-10-14-f10-d-2-front-face-winding-and-culling.md`;
`specs/F10-gamez-mesh-topology-and-material-records.md`;
`specs/F17-rendering-material-fidelity-and-scalable-presentation.md`;
`docs/contracts/IDENTITY-CONTENT.md`.
