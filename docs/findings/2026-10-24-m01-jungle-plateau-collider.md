# M01 jungle terrain near (-5055, -4718): collider present after #1814 (#2032)

Question (from #2021): a ray down at the observed crash point and across
x -7000..-3500, z -4900..-4300 hit only the sea, though the drawn world shows
jungle hills there. Cause, as the owner suspected: `attach_world_scenery` spawned
colliders only for water, so the grouped `g28xxx` scenery had none.

Re-run on main after #1814 (`fa7d99c85`), downward rays from y = 1000 against
the solid scenery triangles of the composed retail M01:

* (-5055, -4718): first hit at y = 321.3 m, node slot 2899 `polys`. Not the sea.
* 100 m grid over x -7000..-3500, z -4900..-4300: 216 of 252 columns meet scenery
  above the sea; the other 36 are open water.

The observed point was in the same frame; no frame mismatch. Test
`accept_m01_scenery_collider_jungle_plateau_is_solid_from_above` (retail,
`crates/cs_app/tests/campaign/m01_scenery_colliders.rs`) pins both; it fails if
the scenery colliders are removed. No code change was needed here.
Not measured: whether the original treats these nodes as solid (see
`2026-10-21-m01-missing-terrain.md`).
