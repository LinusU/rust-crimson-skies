# M01 fire-key kill frame: the muzzle burst does not touch the simulation (#2142)

Question (finding `2026-10-25-m01-retail-red-triage.md`, item 3): after #2106
the `m01_ai_aircraft_damage` fire-key test killed `medkestrel_1` at frame 453
instead of 381; with `stage.muzzle_burst = None` it was 381 again.

## Reproduced (retail, 2026-10-27)

Burst on: kill at loop frame index 452. Burst off: 380.

## Cause: the test's aim point, not the simulation

Dumping the Kestrel's Avian `Position`, the physics, fixed and virtual clocks
for the first updates gave identical values in both runs. The first frame of
the aim loop already differed: the point the test aimed at was 4.0 m further
along -x and 0.4 m lower with the burst off. The test took the
`NodeVisualTransform` of the **first child** of the Kestrel its query met. The
Kestrel has four such children: the swept hull box
(`ScriptedAircraftHullBox`) and three model nodes. Query order follows entity
and archetype layout, which the burst's extra presentation entities shift:

| run | first child met | aim |
| --- | --- | --- |
| burst on | the hull box | box centre, kill at 453 |
| burst off | a model node 4 m off the box | kill at 381 |

No RNG draw, spawn timing or shared buffer is involved; the simulation state
is the same in both runs.

## Fix

The test now aims at the `ScriptedAircraftHullBox` child by name. Burst on and
burst off both kill on frame index 452 (453rd frame), pinned by
`accept_m01_muzzle_sim_retail_kill_frame_ignores_the_burst`
(`CS_GAME_DIR`); `accept_m01_muzzle_sim_hull_box_centre_ignores_sibling_order`
runs in CI. The 453 is the box-aimed kill frame and needs the
`KILL_FRAMES` budget; the 381 was the off-box aim.
