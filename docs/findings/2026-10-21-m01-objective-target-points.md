# M01 objective targets: the record point is the bounding-box centre (#1751)

Date: 2026-10-21. Capability: `retail`. Nothing here is `verified_original`.

## Measured (owner, Drydock, retail 1.02 under the emulator, one setup)

`E` (Next Enemy/Objective) cycles three records, repeating: `[Examine] - Site #1`
(-4278.5, 146.6, -4760.9), `Site #2` (-6311.5, -27.1, -4610.9), `Site #3`
(-8888.5, 20.8, -4686.8); type 8000, side 0. No aircraft are in the list at that time.

## Cause of the difference

Our binding took the node's composed *origin* (`world_transform().translation()`):

| site | node | our origin | original record |
| --- | --- | --- | --- |
| 1 | `t_chamber` | (-4281.5, 125.2, -4753.2) | (-4278.5, 146.6, -4760.9) |
| 2 | `grasshut2` | (17.0, 0.0, 28.3) | (-6311.5, -27.1, -4610.9) |
| 3 | `shipwreck` | (-8891.4, 5.0, -4685.5) | (-8888.5, 20.8, -4686.8) |

The record point equals the node's composed transform applied to the **centre of the
node's stored bounding box**. Each of the three nodes fills exactly one of the three
24-byte boxes of its info record (`unk116`, `unk140`, `unk164`): `t_chamber` `unk140`
centre (2.93, 21.37, -7.62); `shipwreck` `unk164` centre (2.87, 15.84, -1.38);
`grasshut2` `unk164` centre (-6328.48, -27.11, -4639.20) — the authored frame there
is the hut's absolute place under a zero-translation group, so adding the node's own
(16.97, 0, 28.28) gives (-6311.51, -27.11, -4610.92). All three match to 0.05 m.
`grasshut2` is a grass hut (`grasshut2.grass_hut.flt.grasshut`) in a coastal group;
its -27.1 m is its authored place below the 0 m sea plane, and ours is now the same.

## Unknown

* Which box the engine reads when a node fills several (first non-empty is used).
* Why one box slot is used per node; the slot's role is not named by the format.
* Whether the point is recomputed per frame for moving actors (only static
  ground targets use it; actors keep their live pose).

## Consumer

`BoundTarget::target_point_m` → `MissionGroundTarget::target_point_m` → the targeting
store's record position (roster stamp and `sync_mission_target_poses`), which `E`
ranking, the indicator bearing and the distance gates read. The spawned entity stays
at the node origin.
