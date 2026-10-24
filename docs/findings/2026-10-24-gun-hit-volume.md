# Which volume a gun round tests for a grouping ground target (#2061)

Date: 2026-10-24. Capability: `retail`, engine image read statically
(`$CS_ENGINE_IMAGE`). Nothing here ran the original executable; nothing is
`verified_original`. Follow-up of `2026-10-24-node-box-unk164.md` section 3.

## Not proven: the gun-round path

The database-intersection walk (`cls_di.c`, VA `0x4cb950`) was searched for
callers in the image: direct `call rel32` and `jmp rel32` scans of `.text` and
absolute-address scans of the whole file find only the walk's own recursive
calls (`0x4cb64f`, `0x4cb881`, `0x4cba19`). It is reached some other way
(computed or indirect call), so **the path from a fired gun round to that walk
is not established**, and with it which volume (group box, child boxes or
meshes) a round tests. The walk itself narrow-phases a node's box only when
node flag `0x40` is set (`0x4cba00`) and recurses into children otherwise.

## Rule built (inferred)

A target whose stored box encloses another bound target's point is a container
whose box is the union of its children. `mission_targets::drop_container_boxes`
zeroes its extents, so `ground_target_sweep` keeps the designed `[5, 3, 5]` box
(claim `m01-convoy-gun-hits.designed-ground-target-box`) at the unchanged target
point. A leaf target keeps its stored box. The trucks are also flag-`0x40`-clear
grouping nodes, so the flag alone cannot tell them from the bridge; enclosure of
another target does.

Retail M01: `susp_bridge` takes the designed box; `t_truck01..03` and
`bridge_truck01` keep their stored boxes. Tests: `accept_m01_convoy_gun_hits_`
(`a_container_box_does_not_swallow_the_trucks`, and the ignored retail
`retail_container_nodes_do_not_enclose_the_trucks`).

## Still unknown

* Which volume the original's gun hit uses for `susp_bridge` and `rock_zeppelin`.
  Their designed box at the group's centre point is a placeholder, not a measured
  hit volume; whether the original lets gunfire destroy the bridge is unknown.
* The call chain from a gun round to `0x4cb950`.
