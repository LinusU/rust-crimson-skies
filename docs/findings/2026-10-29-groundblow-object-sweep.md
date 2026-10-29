# #2225 GROUNDBLOW-OBJECT-SWEEP: M01's direct-child water tiles are world-object sweep targets

Date: 2026-10-29. Task #2225, a follow-up of #2216
(`2026-10-29-groundblow-sweep-props.md`) and #2217
(`2026-10-29-groundblow-normal-scale.md`). Original-game statements are the
decode's static analysis, never `verified_original`; the port-side numbers are
measured by `accept_groundblow_object_*` against `$CS_GAME_DIR` (M01, group
C3) and by the composed-world probe in `cs_app`.

## The census the task asked for

`world1` (slot 0) has **453** direct-child records in `ZBD/C3/gamez.zbd`. Of
the 347 flagged mesh nodes that bind a water texture
(`2026-10-29-groundblow-class-type.md`'s 347, the `wtr0*`/`water1*` material
rule), **251 are `world1` direct children** and **96 are grouped
descendants**:

| population | count | stored flags | class | spawn path | sweep marker |
| --- | --- | --- | --- | --- | --- |
| `world1` direct children | 251 | `0x0308_831c` | 5 | flat world draw → `WorldColliderInstance` object collider (`node-<slot>`) | `WorldSweepNode` |
| grouped descendants | 96 | `0x0308_031c` | 5 | scenery pass → `PlaytestSea` collider | `PlaytestSea` |

The task's guess — "the 344-vs-347 tile count gap suggests ~3" — measures
wrong on two axes. The gap between the class census's 347 water nodes and its
344 is a **material** distinction, not a placement one: three water nodes
also bind a non-water group — `g28533` (node 571, the only direct child of
the three), `ter_poly` (node 2866) and `craterlake`'s `waterbottom` (node
2996). And the direct-child population is 251, not ~3: most of M01's sea is
drawn as flat world objects, not through the grouping nodes.

`g28530` — node 547, mesh 371 — is an ordinary member of the 251: a
`0x0308_831c` class-5 `world1` direct child.

## Why they are swept

The decision this task recorded is already the merged rule. A direct-child
water tile spawns through `import_world_container`'s flat-object list →
`world::spawn::spawn_mesh_collider` — `Solid` role, `WorldObjectBinding`,
`ColliderFaces`, `BodyLayer(StaticWorld)` — and
`mission_session::world_draw::mark_world_sweep_nodes` (#2216) marks it
`WorldSweepNode` because its stored node carries `0x04|0x10` and a class the
sweep dispatches. `world_sweep` resolves the marker to
`SegmentProbeOwner::World` and tests the node with its own class type.
`accept_groundblow_object_m01_direct_child_water_tiles_are_swept` measures
the composed M01: all 251 tiles present as marked solid `StaticWorld`
colliders carrying `ColliderFaces`, the 96 `PlaytestSea` beside them — the
whole 347 — and a dive at `g28530` answers the sweep as `node-547` with owner
`World`, stopping when the marker is removed.

Two notes for future census work:

* **`PlaytestSea` colliders carry no node slot.** A population measured over
  `WorldSceneryCollider`/`WorldObjectBinding` alone miscounts grouped water —
  #2216's "bound `0x14` nodes below the world root with no collider" list
  names `ter_poly` and `waterbottom`, which do spawn, as `PlaytestSea`. The
  accurate no-collider list is the remainder of that bullet: the `g6`/`o1`
  nodes under `gun`, the `swtr01..05` `g29xxx` groups and the `g28548`
  emitters, none of which are water.
* The draw-side water test (`is_water_image`) is **narrower** than the census
  rule — `water1*`/`wtr00000*` only — and governs `PlaytestSea`/
  `WorldWaterPart`, not the sweep. The sweep's inclusion rule is the stored
  `0x14` flag word plus the class dispatch; texture names decide nothing.

## What is still not tested, and why

Unchanged from #2216: turrets (`TurretBody` has no collider), bound `0x14`
nodes the draw's area selection hides, world-actor nodes and template nodes
outside the world root.
