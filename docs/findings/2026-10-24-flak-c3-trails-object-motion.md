# FLAK-C3-TRAILS (#1925): what `OBJECT_MOTION` does to the `flak_trailN` objects

Date: 2026-10-24. Follow-up of `2026-10-22-flak-c3-burst-fx.md`. Capabilities used:
a static read of `$CS_ENGINE_IMAGE` (disassembly kept in `private/`, never
committed) and a read-only look at `flak_trails.zrd` in `ZBD/zrdr.zbd` of
`$CS_GAME_DIR`. No original executable ran: every claim is provenance
`static`, none is `verified_original`. Implementer: `claude-1` (Sonnet 5.5);
no independent review yet.

The cited instructions are the rows `[[object_motion_fact]]` of
`data/owner-static/engine-facts.toml` (claim `OWNER-STATIC-2026-10-24`), each
with its VA, machine-code bytes and the fact it decides. The acceptance tests
`accept_flak_c3_trails_*` re-read every row against the image (ignored without
`CS_ENGINE_IMAGE`). Names in `code` below are the rows' `name`.

## What is stored

`flak_trails.zrd`, animation `call_flak_trails`: five `OBJECT_MOTION` events
for `flak_trail1..5`, e.g. trail 1:

```
GRAVITY (LOCAL -3)  TRANSLATION_RANGE_MIN (35 60 18 0)
TRANSLATION_RANGE_MAX (55 70 25 0)  FORWARD_ROTATION (TIME 40 0)  RUN_TIME 2
```

then `OBJECT_ACTIVE_STATE INACTIVE`. Each object has a `squeezpuffer` at
`AT_NODE flak_trailN` (`DISTANCE_INTERVAL 2.5`, `WORLD_VELOCITY (0 3 0)`, random
velocity ±1, `FRICTION`, `SIZE_RANGE`, `LIFETIME_RANGE`, `thickblksmoke01..03`).

## Measured: the parser

The `OBJECT_MOTION` parser fills a 0x14c-byte record (`ENGINE_EVENT_KEYWORDS`
row 10): `GRAVITY` sets flag `0x1` (`parse.gravity_flag`); `GRAVITY LOCAL <real>`
stores the real at `+0x18` (`parse.gravity_local_store`; the token `DEFAULT`
would instead store the run-time global `0x9fd164`, which no stored trail
uses). `TRANSLATION_RANGE_MIN` sets flag `0x8` and stores its four words at
`+0x20, +0x28, +0x30, +0x38`; `TRANSLATION_RANGE_MAX` sets flag `0x10` and
stores them at `+0x24, +0x2c, +0x34, +0x3c` (`parse.range_*`). `RUN_TIME` sets
flag `0x400` and stores at `+0x148` (`parse.run_time_*`).

## Measured: the executor `0x4e8fa0`

* **Setup, once.** A byte at event-instance `+0x20` gates the setup block
  (`exec.setup_gate`); with flag `0x8` it makes four draws. Each draw is
  `table[idx]` of a 200-entry table at `0x9fce20`, `idx = (idx + 1) mod 200`,
  and the value is `MIN + (MAX - MIN) * draw` (`exec.draw_heading`). The table
  is filled from `rand()` times `0x38000100` (1/32767), so a draw is in
  `[0, 1]` (`table.fill`, `table.fill_loop`).
* **The four words.** Word 0 is a **heading in degrees**
  (`exec.heading_to_radians`: times the double `pi/180`); the heading vector is
  `(-sin h, 0, -cos h)` (`heading.sin_cos_negated`, `heading.negate`), so
  heading 0 points to **-Z**. Word 1 is an **elevation in degrees**,
  `e90 = elevation / 90` (`exec.elevation_scale`). The direction is
  `(-sin h * T, e90, -cos h * T)` with `T = 1 - e90` for `e90 >= 0` and
  `1 + e90` otherwise (`exec.horizontal_factor`, `exec.direction_store`): a
  **linear** horizontal share, not `cos(elevation)`, so the direction is not
  unit length. Word 2 multiplies it into the **initial velocity**, word 3 into a
  constant **acceleration** (`exec.speed_store`, `exec.launch_copy`); all four
  stored trails have word 3 = 0.
* **Gravity.** `GRAVITY LOCAL g` (without the `COMPLEX` token, flag `0x2000`)
  adds `g` to the acceleration's second component once at setup
  (`exec.gravity_local`). `g` is in the same units as the velocity words
  (stored -1..-3; the executor applies no further factor).
* **Each frame.** The step is the frame time; with `RUN_TIME` set, a frame whose
  event time `+0x28` exceeds `RUN_TIME` is trimmed by the overshoot
  (`exec.step_dt`), and the executor reports done once `+0x28 >= RUN_TIME`
  (`exec.run_time_end`). The object moves by `step * velocity`
  (`exec.step_displacement`) through `0x4d27d0` / `0x4d1e50`, which add the
  vector to the node's own translation fields without rotating it by the node's
  orientation (`exec.translate_call`, `exec.translate_add`); then
  `velocity += step * acceleration` (`exec.velocity_integrate`). Displacement
  uses the velocity from before the frame's acceleration.

So an object flies exactly `RUN_TIME` seconds along `p += dt v; v += dt a`.
Trail 1 launches at 18..25 m/s with elevation 60..70 degrees, i.e. mostly
upward (`e90` 0.67..0.78, horizontal share 0.22..0.33), and gravity -3.

## Units and frame

Positions are the world's units, which the project reads as metres everywhere
else (e.g. the burst puffs); the executor applies no unit factor, so speeds are
units per second and `g` units per second squared. The motion's frame is the
node's parent frame (`exec.translate_add`); what that is for an animation
started at a position with no node is the unresolved FLAK-C1 item.

## Drawn

`flak_burst_fx.rs` reads `call_flak_trails` (`read_flak_trails`), and at each
burst throws every object with `trail_launch` / `TrailBody::step` (fixed ticks,
trimmed last tick), emitting a burst puff (#1806 art) each `DISTANCE_INTERVAL`
metres moved. `FlakBurstFxReport` counts `trails_thrown`, `trail_puffs_spawned`;
`trails_undrawn` now counts only bursts with no trails installed. The owner
smoke does not show them: no flak shooter fires in M01 yet (FLAK-C2/C5).

## Designed (claim `flak-c3-trails.designed-draws-frame-and-puffs`)

* The draws come from a deterministic generator seeded from the burst (the
  original uses `rand()`); the table's unit range is measured, its sequence is
  not reproducible.
* The motion frame is the world's, with the burst point as origin and no
  `CALL_SEQUENCE ... AT_NODE (0 3 0)` offset (those offsets, stored on trail 1 and
  3 as `(0 3 0)` / `(0 5 0)`, are unmeasured).
* One puff each time the object has moved `DISTANCE_INTERVAL` metres, none at
  the start; the puff's velocity is `WORLD_VELOCITY` plus a draw between the
  random velocities; its texture is one of the burst puff's three. The
  `TEXTURE_SEQUENCE` times, `GROWTH_FACTOR`, `FADE_RANGE` and
  `DEVIATION_DISTANCE` are not applied (same as #1806).

## Not drawn, unknown

* **`FORWARD_ROTATION`** (`TIME 40 0` on trails 1 and 3; `TIME` sets flag `0x80`, `DISTANCE` `0x40`):
  the executor turns the object about its own axes by an angle proportional to
  its motion. The puffer sits at the object's origin, so no puff moves; whether
  the objects carry visible geometry is **unmeasured**, so the objects
  themselves are not drawn.
* **The ground clamp.** For a motion with neither flag `0x4000` nor `0x8000`
  (all five trails), a falling object is tested against the terrain
  (`0x4e9e30`) and held at the ground; its reach and units are unmeasured, so
  a trail that would meet the ground or the sea keeps flying.
* The setup byte's setter at instance `+0x20` was not located; "once" is
  inferred from the block drawing random numbers.
