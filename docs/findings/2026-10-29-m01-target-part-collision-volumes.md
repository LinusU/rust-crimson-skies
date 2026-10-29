# M01 target parts: which collision volume each carries, and the swept box narrowed to it (#2211)

Date: 2026-10-29. Follow-up of `2026-10-29-hit-record-contact-point.md` (#2208, the
`[[hit_record_fact]]` rows). Capability: `retail` (read-only `$CS_GAME_DIR`) and a static read
of `$CS_ENGINE_IMAGE` (Capstone). No original executable ran; nothing here is
`verified_original`. Test: `accept_part_volume_` in
`crates/cs_app/tests/campaign/m01_part_volume.rs` (retail, `#[ignore]`).

## Which volume a node carries

The runtime node is the 36-byte name followed by the 208-byte info record
(`docs/findings/2026-09-29-f10-b-gamez-mesh-layout.md`: `mesh_index` at offset 60), so the
engine's `[node+0x24]` is the stored `RawNodeInfo::flags` word and `[node+0x3c]` is the slot
of `mesh_index`. Evidence in the image:

| fact | address |
| --- | --- |
| the contact query rejects a node unless `[node+0x24]` has bits `0x4` **and** `0x10` | `0x4c9a06..0x4c9a19` |
| a leaf class handler tests the polygon soup `[node+0x3c]` with `0x55c9c0`, nonzero only | `0x4c9ba2..0x4c9bc9` |
| flag `0x20` reaches `0x55d270` at exactly one call site, inside the node-bound reject routine, with a **stack-local scratch record** and a reject-on-miss return | `0x4cc1be..0x4cc1ef` |

So a node with flag `0x20` clear — every node below — is narrow-phased against its polygon
soup, and flag `0x20` is a bound gate rather than a stored contact face. (This refines #2208,
which listed the `0x55d270` box as the contact surface for flag-set nodes; no M01 target part
sets the flag, so the M01 numbers do not depend on the difference.) The loader step that turns
`mesh_index` into the `[node+0x3c]` pointer was **not traced**; that nonzero `[node+0x3c]`
means `mesh_index >= 0` is the reading of the two layouts above.

## Retail measurement (`ZBD/C3/gamez.zbd`, the M01 world)

No bound target, and no node below `t_truck01..03`, `bridge_truck01` or `shipwreck`, has flag
`0x20` set: the flag words are `0x308051c` (grouping nodes), `0x308031c` / `0x308071c` (leaf
parts), `0x308050c` (the `destroyed` node), `0x308030c` (`shadow`, `flashlight`) and
`0x308001c` (`firepoint`).

| node group | mesh | flags | volume |
| --- | --- | --- | --- |
| `t_truck01..03`, `bridge_truck01` (the targets) | none (`-1`) | `0x...51c` | grouping node; box `unk164` only |
| `stude_truck`, `turret`, `gun`, `chasis`, `tank` | `462..470` | `0x...31c`, `0x...71c` | **polygon soup**, bound `unk140` (`turret` `unk116/140/164` all stored) |
| `destroyed`, `shadow`, `flashlight` | none / `468`, `471` | `0x...50c` / `0x...30c` | not collidable (bit `0x10` clear) |
| `firepoint` | none | `0x...01c` | no volume |
| `shipwreck` hulls, masts, railings | `452..461` | `0x...31c` | polygon soup, bound `unk140` |

The stored bound of every collidable leaf contains its soup: the test asserts every vertex of
every collidable leaf mesh of the four convoy trucks lies inside the node's stored box.

## The runtime change

`BoundTarget::contact_box_m` is the world-axis union of the stored bound of every collidable
leaf (mesh present, flags `0x4`+`0x10` set, `0x20` clear) among the target's intact slots, and
`spawn` sweeps against it instead of the grouping node's box. The HUD point and extents
(`target_point_m`, `target_half_extents_m`) are unchanged. A target whose extents
`drop_container_boxes` zeroed, or one re-seated under a moving actor, keeps the previous box.

Measured effect (world metres; the test prints these):

| target | box | old (grouping) half | new (leaf union) half |
| --- | --- | --- | --- |
| `t_truck01` | x / y / z | 1.568 / 2.313 / 3.635 | 1.533 / 2.313 / 3.634 |
| `t_truck02` | x / y / z | 1.908 / 2.313 / 3.753 | 1.867 / 2.313 / 3.748 |
| `t_truck03` | x / y / z | 1.597 / 2.313 / 3.646 | 1.555 / 2.313 / 3.645 |
| `bridge_truck01` | x / y / z | 1.921 / 2.935 / 8.783 | 1.833 / 2.173 / 8.783 |

`SweptHit::time_of_impact` on `t_truck01`, a 40 m tick (`wep_12`: 1200 m/s at 30 Hz) through
the box centre along each world axis: the entry time moves from 0.45994 to 0.46167 along `-x`
(**0.069 m** later along the segment), 0.40911 to 0.40916 along `-z` (0.002 m), and not at all
along `y` or from the `+` sides. The tangler field centre (`rocket+0x48 = record+0xc`) moves by
the same amount: at most about 0.07 m on the trucks. On `bridge_truck01` the top face drops
from y = 124.28 to 122.80 (**1.48 m**, about 3.7 % of the 40 m tick) for a round from above.

## Residuals (unresolved)

* The sweep is still a box. Soup triangles are not swept: the leaf bound is only the first
  narrowing, and a round that enters a leaf bound but misses its soup is still a runtime hit
  the original does not produce. No such false positive was measured on these targets (the
  soup-to-bound gap was not quantified, only the containment).
* The player's wingman Fox 2 is an aircraft whose parts live in `planes.zbd`, not the world
  container; its parts' volumes were **not measured** here.
* Effect-only surfaces (`[surf+0x20] == 1`) were not looked up: the per-face surface table of
  the soup meshes was not read, so whether any M01 part carries one is unknown. No runtime
  handling was added.
* First-accepted-face ordering inside a node (`0x55d31e`) is not modelled.
* The leaf union of a multi-leaf target is one box; the original picks the nearest face across
  the leaves, which can be deeper than the union's entry face for oblique rounds.
