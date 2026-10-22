# FLAK-C3-BURST-FX (#1885): the `flak_effect` presented at a flak burst

Date: 2026-10-22. Builds on `2026-10-21-parity2-dmg-fx-a-effect-definitions.md`,
`2026-10-21-parity2-dmg-fx-b-burst-puffs.md` and
`2026-10-22-flak-c1-burst-presentation-and-descriptor-flag.md`. Capability used:
`retail` (read-only `$CS_GAME_DIR`). No original run; nothing is
`verified_original`.

## What is drawn

`crates/cs_app/src/mission_session/flak_burst_fx.rs` reads `flak_effect`
(`flak_control.zrd`, `ZBD/zrdr.zbd`) at stage time (M01) and presents an
`OrdnanceBurst` of the default `IMPACT` row (`impact_row == 0`) published on
`PendingFlakBursts`:

| Part | Stored | Presentation |
| --- | --- | --- |
| Orange flash | `LIGHT_STATE flak_light1`: `RANGE 4 10`, `COLOR 0.95 0.76 0.24`; `LIGHT_ANIMATION RANGE 16 40 0 0` over 0.3 s, then `-14 -34 0 0` over 0.1 s | point light at the burst, range animated, released after 0.4 s |
| Smoke ball | `CALL_ANIMATION large_black_smokeball` | handed to the #1806 burst puffs as a `PuffBurst` at the burst point |
| Flicker | `IF PLAYER_RANGE 100` calling `frame_buffer_effects1`: six `FBFX_COLOR_FROM_TO` steps, 0.2 + 0.4 + 0.2 + 0.3 + 0.2 + 0.2 = 1.5 s | full-window colour overlay, only when `distance² <= 100²` from the burst to the player body (FLAK-C1's compare) |

Nothing publishes flak bursts into the mission yet: the round's owner is
FLAK-C2 (#1821); this module is its consumer.

## Designed (claim `flak-c3.designed-flash-and-flicker-curves`)

The data states no unit or curve. Designed: the second `RANGE` number is the
point light's range; each `LIGHT_ANIMATION` adds its first two numbers
linearly over its `RUN_TIME` (words three and four are unmeasured, ignored);
the light's intensity; each flicker step blends its stored RGBA linearly over
its `RUN_TIME`; the flicker distance is measured from the burst point and it
starts with the burst.

## Not drawn

* **`call_flak_trails`** (five `flak_trail1..5` objects with `OBJECT_MOTION`:
  `TRANSLATION_RANGE_MIN/MAX`, `GRAVITY LOCAL -1..-3`, `FORWARD_ROTATION`,
  `RUN_TIME` 2-3 s, each with a `squeezpuffer` along its path). The units and
  frame of `OBJECT_MOTION` are unmeasured. `FlakBurstFxReport::trails_undrawn`
  counts every burst without them.
* The unresolved items of FLAK-C1 (which code path starts the row animation,
  what `[anim+0x6c]` is for a node-less start, when the `IF` is evaluated, the
  runtime of `FBFX_COLOR_FROM_TO`): the timing and distance origin above are
  designed, not measured.
* Not shown in the owner-command smoke: no flak shooter fires in M01 yet
  (FLAK-C2/C5), so there is no framebuffer of this effect.
