# F10-F.3: FrontFaceWinding retired from the mesh presentation unknown list

Task #1409, follow-up of F10-D.2 (#396).

The front-face winding was measured from the original executable
(counter-clockwise front, `OWNER-STATIC-2026-10-05`; see
`2026-10-14-f10-d-2-front-face-winding-and-culling.md`). Claim class: static
code evidence, never `verified_original`.

## Changed

* `cs_content::mesh::mesh_presentation_unknowns` and
  `cs_app::world::retail::stored_presentation_unknowns` no longer emit
  `MeshPresentationUnknown::FrontFaceWinding`, so render-mesh catalog rows no
  longer carry `front_face_winding_unknown`. The variant stays (documented as
  measured) so the closed vocabulary does not silently shrink.
* The GPU capture still draws with `cull_mode: None`; its rationale is now that it
  is a geometry witness, not that the winding is open.

## Still open (unchanged)

`UvOrigin`, `VertexColor`, `MultiMaterialGroup` (when the mesh stores such a
polygon) and `MaterialUnknown::TwoSided` remain unknown.

## Tests

`accept_f10_f_3_*` (one in `cs_content`, one in `cs_app`) fail if the entry is
put back.
