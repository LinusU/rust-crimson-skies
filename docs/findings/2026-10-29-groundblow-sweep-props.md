# #2216 GROUNDBLOW-SWEEP-PROPS: which of M01's non-scenery colliders the ground-blow sweep tests

Date: 2026-10-29. Task #2216, a follow-up of #2214
(`2026-10-29-groundblow-sweep-fill.md`). Original-game statements are the
decode's static analysis, never `verified_original`; the port-side numbers are
measured by `accept_groundblow_props_*` against `$CS_GAME_DIR` (M01, group C3).

## What the composed M01 holds

Container nodes that store `flags & 0x14 == 0x14` and bind a mesh: 2393. Where
the port puts a collider for them (headless composition, one `app.update()`):

| source | colliders | in the sweep before | now |
| --- | --- | --- | --- |
| scenery pass (`PlaytestSea` 96, `WorldSceneryCollider`) | incl. ground-target `healthy`/`dbase` nodes, `fuel_truck01` `chasis`/`tank`, towers, units, the gun emplacement's `g20`/`pt*` | yes | yes |
| **world-definition flat objects** (`WorldObjectBinding`, `StaticWorld`) | **374**: 367 × `0x0308831c`, 7 × `0x0308871c`, all class 5; the `g28xxx` tiles and the `zepbridge1/2` props | **no** | **yes** |

The 374 are the "StaticWorld bodies with their own colliders and no scenery
marker" of the task. Each is marked `WorldSweepNode { node_slot, class_type }`
by `mission_session::world_draw::mark_world_sweep_nodes` when its stored node
stores `0x04|0x10`, a class the sweep dispatches (`class_type_intersects`) and
the collider is `Solid`; `world_sweep` resolves a marked collider to
`SegmentProbeOwner::World` and tests it with the node's own class type. The
port's synthetic ground and obstacle (`PlaytestGround`, `PlaytestObstacle`) and
any unmarked `StaticWorld` body stay out.

## What is still not tested, and why

* **Turrets.** `TurretBody` (`turret_fire.rs`) is a transform and a weapon
  binding with no collider, so there is nothing to sweep. The `gun`/`turret`
  polygons of #2212's census (`gun` 137, `turret` 80, `hull` 47) belong to
  nodes the port does not spawn as colliders (below).
* **Bound `0x14` nodes below the world root with no collider** (M01): the
  `g6` nodes under `gun` (g28515 ×2, g28707, barracuda ×2) and `o1`, the
  `swtr01..05` `g29xxx` groups, `craterlake` `waterbottom`/`waterlevel`,
  `ter_poly`, the emitters under `g28548`. They are left out of the draw by the
  area selection (an LOD band or damage alternative that is hidden, or a flat
  card), so the scenery pass never spawns a collider for them. Whether the
  original sweeps the hidden alternative is not measured here.
* **World-actor nodes** (`piratezep`, `cargozep1`, ...): drawn by the actors,
  skipped by the scenery pass, no static collider.
* **Template nodes outside the world root** (311 bound `0x14` nodes): not
  placed in the world.

Resolving the hidden alternatives and the turret meshes needs the draw
selection and the turret body to carry geometry; that is a separate task.
