# The escape-climb probe over the partition's members (#2292)

Date: 2026-11-02. Follow-up of #2290
(`2026-11-02-ai-terrain-query-4c8f70.md` §9-§12) and #2281. Implementer:
`sonnet-1`. `static` and retail claims only; the original was not run, nothing
here is `verified_original`.

## What changed (behind `MissionAircraftAi::original_stick`, default off)

* `ESCAPE_WORLD_CEILING_M` is **8000.0**: `0x474151` is
  `c7 05 f4 c3 71 00 00 00 fa 45` (`mov [0x71c3f4], 0x45fa0000`), the floor
  store `0x47415b` is `... a0 41` (20.0). Pinned by
  `accept_m01_escape_probe_grid_ceiling_is_the_image_word`.
* `WorldSegmentProbe` takes `Option<&AreaPartitionMembers>`
  (`crates/cs_app/src/playtest/world_sweep.rs`). The set is read from the
  mission world's own node array (`read_area_partition_members`, once the
  stage carries `world_draw`): a node whose `AreaPartitionPm` is not
  `(-1, -1, ..)` is listed (retail C3: exactly the 439 entries), plus the
  listed nodes' descendants. A collider without a node slot (aircraft) is not
  a scene node and is not tested. The runtime look-ahead passes the set; the
  ground-blow sweep is unchanged.
* The per-object test is still the sweep's face rule on the collider, not the
  original's AABB overlap `0x4cc010` / exact `0x55c9c0`: **approximation**,
  those tests stay undecoded.

## Correction to §11: the ring and the cliffs are reachable through their groups

§11 says the ring `g28637` (slot 2994) and the east cliffs `g28911` (2965) are
in no cell, so the query "can never return them". Their own
`AreaPartitionPm` is the default, but so is the hillside's mesh node 2882
`g28982`. Measured in C3's node array:

| slot | name | type | mesh | `AreaPartitionPm` | parent |
| --- | --- | --- | --- | --- | --- |
| 2874 | `g28604` | 5 | -1 | (12, 4) | world |
| 2882 | `g28982` | 5 | 762 | default | 2874 |
| 2985 | `craterlake` | 5 | -1 | (7, 4) | world |
| 2994 | `g28637` (ring) | 5 | 795 | default | 2985 |
| 2963 | `g28609` | 5 | -1 | (8, 4) | world |
| 2965 | `g28911` (cliffs) | 5 | 776 | default | 2963 |

The hillside's mesh is a leaf under a listed group; so are the ring and the
cliffs. The cell lists the group, with the subtree's y range, and the object
test recurses into children (type 5, `0x4c9adf`). Reading the group as
covering its subtree (needed for the hillside to be seen at all) therefore
also reaches the ring and cliffs; the membership data cannot separate them.
Whether the original really walks the children of a *listed* type-5 group in
the AI query is the open point (the exact tests `0x55c9c0`/`0x4cc010` are
undecoded). The port reads the subtree; `AreaPartitionMembers::is_listed`
keeps the direct reading available.
`accept_m01_escape_probe_grid_membership_reads_the_retail_partition` pins both.

## Re-fly

`accept_m01_wingman_stick_betty_over_the_hillside` (retail M01, 45 s, 120 Hz,
`original_stick` on, ceiling 8000, probe over the members; the test asserts the
set is installed):

| gains | closest | worst after 30 s | final | latched |
| --- | --- | --- | --- | --- |
| merged (0.6 / 0.0 / 0.15, 100 / 100 / 0.25) | 84 m | 577 m | 577 m | 193 / 5400 |
| `ai_input_*` (3.5 / 1.0) | 119 m | 4131 m | 4131 m | 4687 / 5400 |

Identical to the run before this change (`2026-11-02-m01-wingman-gains.md`
§8): station is **not** held, so `original_stick` stays off. A direct-only
reading (excluding the mesh leaves) was not flown: it would hide the hillside
itself.

| unknown | affects | resolving step |
| --- | --- | --- |
| does the AI query recurse into a listed type-5 group's children | whether the ring/cliffs are hit | decode `0x4c9adf` children loop for this call |
| exact contact tests `0x55c9c0`, `0x4cc010` | hit vs AABB overlap | decode |
