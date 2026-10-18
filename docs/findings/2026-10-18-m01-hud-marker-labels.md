# M01 HUD: hostile marker labels (#1658)

Date: 2026-10-18. Follow-up of #1641. Capability: `retail` (owner-command smoke).

## Measured / used

* Aircraft markers use the display name of the executable's airframe table
  (`mission_start::AIRFRAME_TABLE`, `0x620c70`, #715): the nearest entry of the
  actor's `vehicle.zrd` `kind_of` chain that spells a row's display name, scene
  root or model gives the row's display name. `medkestrel_*` now reads `Kestrel`,
  as in the original capture.

## Unknown (not guessed)

* The original's on-screen name for **zeppelins** (`cargozep1` etc.): no measured
  source. The marker keeps the record id until a source is found.
* Font metrics and the original's label offsets: label width is a designed
  estimate (`LABEL_CHAR_WIDTH`), the reticle clearance square is designed.

## Layout

Every marker label goes through `cockpit_hud::place_label`: clamped inside the
window, kept out of a square around the reticle, and moved down (else up) so no
two labels overlap. Owner smoke screenshot (not committed):
`private/hud-markers/frame_00300_hud.png`.
