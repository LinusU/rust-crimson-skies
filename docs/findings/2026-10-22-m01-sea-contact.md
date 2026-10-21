# #1850: the sea as a contact source for the player in M01

Date: 2026-10-22. Implementer: `claude-2`. Evidence: the owner's `zbd/c3/gamez.zbd`
read through production readers (`retail`, no original run) plus the static
reading in `2026-10-21-m01-contact-response-static-locate.md` (`OWNER-STATIC`).
Nothing here is `verified_original`.

## What the container says about the water tiles

The static reading found no sea plane in the original's world sweep: a contact
exists only for a node whose flag word (`node+0x24`) has bits `0x04 | 0x10`.
Reading `c3` with `RawNode::flags()`: the 347 nodes that bind a mesh whose every
material group is `wtr0xxxx` or `water1` store `0x0308831c` (251, indexed by the
partition grid) or `0x0308031c` (96, not indexed). **Both carry `0x04 | 0x10`**
and neither carries `0x40`. So the sweep tests M01's water tiles like any other
mesh node, and the sea is a contact through the tiles' own triangles at their
own altitude (y = 0 for the flat sea, up to 13 m for pools and falls), not a
plane. What `0x4cad00` does for the node's type (`node+0x34`) is still not
decoded; the tiles are tested here as triangle meshes.

## Why we had none

The tiles hang below grouping nodes of the world record, so the flat world
definition does not hold them (#1735). They were drawn as scenery
(`WorldScenery`) and never became colliders.

## What changed

* `WorldScenery` keeps each mesh's collision view (`colliders`) and each
  placement its node's flag word (`node_flags`).
* `attach_world_scenery` spawns a static trimesh collider (world layer, the
  mesh's canonical collision topology, #1424) for each placement that is a
  water tile and stores `CONTACT_SWEEP_NODE_FLAGS` (`0x14`), marked
  `PlaytestSea`; a contact with it counts as a ground contact in the HUD.

## Not done

* Only water tiles. The rest of the scenery (beach, land) stores the same
  flags; making it solid is the contact-response work (#1752), which also owns
  what the contact *does*. Here the solver holds the body at the surface; the
  original's response (skim and bounce, the damage pair) is not reproduced.
* Whether the original's sweep reaches the 96 non-indexed tiles through the
  node list or not at all is unknown; all water tiles with the flags are
  colliders here.
* The airframe's `collision` points (the original sweeps points, we sweep our
  cuboid) are unmeasured.

## Test

`accept_parity4_m01_sea_contact_a_low_body_skims_the_sea` (ignored, needs
`CS_GAME_DIR`): composes M01 headlessly, puts the player body 1 m above a flat
water tile on a -2.8 deg path at 113.5 m/s, and asserts a ground contact, no
tunnelling below -3 m, a surviving body and no reset. Verified to fail with the
collider spawn disabled.
