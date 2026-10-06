# #665: the complete intact Bloodhawk in the retail playtest

Date: 2026-10-05. Task: #665 `PLAYTEST-FULL-AIRCRAFT`. Owner feedback: "my
bloodhawk seemed to be missing its wings". Capabilities: **`retail`** and **`gpu`**
(`Apple M3 Pro`, Metal). Render evidence only, not `verified_original`; the frame is
under `private/evidence/PLAYTEST-FULL-AIRCRAFT/` and nothing original is committed.

> **Superseded (2026-10-06, #709):** "the designed nose half turn" below is gone:
> the propeller is at `bloodhawk`'s tail, the measured stored nose is `−Z`, and the
> nose mapping is now the identity. See
> `docs/findings/2026-10-06-t709-airframe-nose-mapping.md`. The selection rule and
> every count here stand.

## What the airframe holds (measured, production readers)

`bloodhawk` (root slot 2363) has 40 mesh bindings in these subtrees:

| subtree | slot | content |
| --- | --- | --- |
| `healthy` | 2296 | six `Lod` bands: `nearest` 0-150 m (fuselage halves `g442`/`g443`, both wings and ailerons, elevators, rudder, canopy, pilot, two wing panels `pdp2i`/`pdp3i`), `l12` 0-50 m (four engine meshes), `l3` 150-250, `l5` 250-450 (`fuse03`, the #648 mesh), `l7` 450-1500, `l8` 1500-2500 (`g59`, 0 triangles) |
| `shadow` | 2534 | one flat quad |
| `destroyed` | 2535 | four wreck pieces |
| `dontmove` | 2540 | six propeller meshes |
| `markers` | 2547 | no meshes |

The node flag bits are unmeasured, so no stored flag says which of these is the
intact set, which propeller is shown when, or what `l12` is.

## The selection rule (`select_aircraft_parts`)

1. The intact subtree is the **pinned** `healthy` node (slot 2296, name-checked on
   every spawn, like the area slot). Everything outside it is not the intact set.
2. A `Lod` band whose range lies inside a wider sibling band is a **detail overlay**,
   not a variant of the same part. Measured: `l12` (0-50 m) is nested in `nearest`
   (0-150 m) and holds only the engines. The F11-B `select_lod_variant` rule applied
   to every sibling would choose the tightest band, `l12`, and draw **no wings**; so
   overlay bands are not candidates. This is a designed reading, not an original rule.
3. `select_lod_variant` chooses among the remaining bands at the designed 20 m
   viewer distance: `nearest`, `Covered`.
4. Every mesh binding under `nearest` is drawn at its own composed transform; the
   other bands, and everything outside `healthy`, are listed with a reason.
5. One propeller, `staticprop1` (slot 2541, picked by authored name), is added.

Result (`playtest sources`): `aircraft_mesh_bindings` 16, `aircraft_triangles` 927,
`aircraft_airframe_bindings` 40, 24 undrawn: `bheng01-04` (overlay band), the
`l3`/`l5`/`l7`/`l8` bindings (`fuse02`..`g59`), `shadow`, `piece1-4`, and the five
other propeller meshes. Nothing falls back to the fuselage alone: a binding that will
not build is reported and the rest are drawn, and a set with no drawable mesh is an
error.

## Placement and collider

Every part is a child of the one flight body with its composed canonical transform
(`SceneNode::visual_transform`, converted by `NodeVisualTransform`) under the one
designed nose half turn. The Transform of a part is written once, at spawn; the
pose owner moves the body. `R` despawns the body and its parts together.

The collider is **one box** measured from the composed extent of the drawn set
(`SceneNode::world_transform` corners), centred on the body. The extent is
about 11.6 m span x 2.7 m x 10.6 m, so the box is wider than the previous
fuselage-only one. The box is not fitted to the mesh's centre (it sits about 0.5 m
off the body origin along the nose axis).

## GPU evidence

`accept_playtest_full_aircraft_gpu_chase_view_shows_wings_beyond_the_fuselage`
renders the `chase` view three times (whole aircraft, fuselage halves only, no
aircraft) and differences them: 11 916 aircraft pixels, 10 170 of them the fuselage
halves' footprint, **1 746 outside it**.

## Still provisional

- Static propeller, no moving control surfaces; the propeller choice reads a name.
- LOD band fixed at a designed 20 m; the original's LOD selection is unmeasured.
- `l12` engines, `pdp2i`/`pdp3i` damage panels (drawn: they sit under `nearest`) and
  the damage/wreck states have no measured meaning.
- One neutral material, no textures (PLAYTEST-TEXTURES).
- Single box collider.
