# F27-C-FX-ORIG-ENGINE: the engine reads the weapon row's FIRE > ANIMATION

Date: 2026-10-25. Task #2060, follow-up of #1684. Capabilities: `retail` +
`CS_ENGINE_IMAGE`; static reading only, nothing here ran the original executable
and nothing is `verified_original`. Rows: `[[weapon_fire_animation_fact]]` in
`data/owner-static/engine-facts.toml` (claim `OWNER-STATIC-2026-10-25`, provenance
`static`). Tests: prefix `accept_f27_c_fx_orig_engine_` in
`crates/cs_app/tests/campaign/f27_c_fx_orig_engine.rs`. No disassembly committed.

## Confirmed: weapon fire plays the FIRE ANIMATION named in the weapon row

- The weapon row parser (`0x5ae0fc`) hands the key `FIRE` to the block reader
  `0x5ae990`, destination `weapon+0x94`. The reader's `ANIMATION` key
  (`0x5aeac4`) stores up to two animation handles at `weapon+0x98` and
  `weapon+0x9c`. The same block holds `EFFECT` (`+0xb8`), `MODEL` (`+0xbc`) and
  the sound lists (`+0xc4`; the shot sound already recorded as `play.weapon_fire`).
- The weapon fire routine `0x5aef9c` plays the shot sound, then: if `EFFECT`
  (`weapon+0xb8`) is non-zero it plays that and skips the animation; otherwise,
  when the fire index `[weapon+0xec]` is not -1, it starts the FIRE animation
  handles (`0x5af402..0x5af4e5`): index 0 starts both slots, index n starts slot
  n-1. A zero handle is skipped. If block flag bit 1 is set and `[weapon+0xf0]` is
  non-zero the animation is started attached (`0x4edf50`), otherwise at the muzzle
  position with a yaw/pitch from the muzzle direction (`0x4edc10`).
- So the data-only binding "FIRE>ANIMATION -> muzzle_burst.zrd" of #1684 is the
  path the engine takes, for a row that has no FIRE>EFFECT.

## EVENT_OFFSET and the AT_NODE offset

- `EVENT_OFFSET` is stored as a float at event+8, unscaled (integers via `fild`);
  its **unit is not decided by the parser** and the scheduler consumer was not
  traced. **Unknown** (seconds vs ticks), affecting every animation sequence that
  uses `EVENT_OFFSET`, including `muzzle_burst`.
- `CALL_ANIMATION AT_NODE` stores the node index and the offset floats
  (`record+0x38`, `+0x44`) with no scaling, so (0,-0.2,-1) is the written value.
  The **node z-axis / mount frame** that consumes `record+0x38` was not traced.
  **Unknown**, affecting the muzzle burst's placement.

## Open follow-up

Trace the animation scheduler consumer of event+8 and of record+0x38 (callers of
`0x4ef7d0` results); until then the existing measured placement stays provisional.
