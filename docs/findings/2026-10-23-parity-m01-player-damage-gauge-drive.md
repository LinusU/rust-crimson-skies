# PARITY-M01-PLAYER-DAMAGE-GAUGE-B: driving the cockpit damage zones

Task #1943, building on GAUGE-A
(`2026-10-23-parity-m01-player-damage-gauge-mapping.md`). Provenance: `static`
(never `verified_original`).

## Resolved: what `RESET` indexes (GAUGE-A unknown 1)

Engine image, handler for event id 17 `OBJECT_CYCLE_TEXTURE` at `0x4eabd0`
(static read, disassembly not committed): it returns after doing nothing unless
the record's flag word at `+0x0c` has bit 0 set (`RESET` present) and the
node index at `+0x10` is non-negative. It then calls `0x5642a0` (rewind every
cycle of the node's material) and `0x564410(material, (int16)[record+0x12])`.
`0x564410` sets the current frame to `table[n]` with `n` taken **0-based**:
`n >= count` wraps (`n mod count`), `n < 0` clamps to 0. So `RESET (1|2|3)` =
yellow, orange, red of the 4-frame `[green, yellow, orange, red]` cycle, and
the value-0 event in `*_damage_green` is the intact green frame. Orange is
reached at the 0.46 `*_damage_yellow` level.

## Driven mapping (`hud_art.rs::gauge_frame`)

Frame = number of ladder thresholds `0.72, 0.46, 0.2` that the zone's remaining
integrity fraction (remaining / declared pool) is at or below; `Destroyed` is
frame 3; `Intact`/`Unknown`/an unresolved pool is frame 0. Each frame swaps
the zone quad's `*hilite` and `*_hatchptrn` to green / yellow / orange / red
(`greenhilite`/`grn_hatchptrn`, `yellowhilite`/`yel_hatchptrn`,
`orangehilite`/`orng_hatchptrn`, `redhilite`/`red_hatchptrn`).

## Still unknown / designed

- Threshold unit (remaining vs damage fraction) and boundary (`<=` vs `<`)
  are inferred from the descending order (GAUGE-A unknowns 2).
- What feeds a part's 0..1 health in the original (unknown 3): the port uses
  the damage resolver's remaining integrity over the declared pool, whose
  pool size is a designed value.
- The `<part>_got_hit` blink (`OBJECT_ACTIVE_STATE` toggling the zone, 8 loops
  of 0.2 s) is not driven.
- The `cockpit_hud.rs` fallback silhouette tint is a different, designed
  element and is left untouched.
- Node resolution of the cycle event (`+0x10`) is not decoded; zones are bound
  by their authored node names.

## Smoke

`cs --cs-path <retail> --mission M01 --smoke-seconds 45` (67 checks, no
failure). The damage-trail phase now hits the player's left wing to 0.40
remaining, nose to 0.65 and tail to 0.15 (right wing untouched; test hits,
listed in `report.json`). `frame_03166_damage_trail.png` shows the gauge dial:
nose tick yellow, left wing orange, tail red, right wing green. Frames are in
`private/evidence/PARITY-M01-PLAYER-DAMAGE-GAUGE-B/` (not committed).
