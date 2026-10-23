# BUG-M01-SHIPWRECK-GRAY-TRIANGLES (#2004): the grey triangles are six tilted `cone` cards

Owner report (human_play, 2026-10-23): huge flat beige/grey triangles stand over
M01's shipwreck. Nothing here is `verified_original`; no original run happened.

## Measured (retail, production readers, `ZBD/C3`)

Census of every drawn M01 part whose material binds no texture (world objects and
scenery), with its composed world-space extent. Six parts match the screenshot:

| node | slots | mesh | position (canonical m) | triangles | extent (m) | material |
| --- | --- | --- | --- | --- | --- | --- |
| `cone` ×6 | 3215, 3218, 3221, 3224, 3227, 3230 | one each | (-8886.5, 85.2, -4678.0) | 1 each | 217 × 272 × 125 or 0 × 272 × 251 | 84 (flat colour) |

They stand 80 m over the shipwreck node (-8891.4, 5.0, -4685.5), six triangles
around one point, the neutral colour (0.74/0.70/0.62) the binder gives a record
that names no texture — the beige of the screenshot. They are the same class as
the C1C landing cards of `2026-10-08-playtest-area-flat-shape.md`: one polygon,
flat material, stored as scenery helpers (the nodes store flags `0x308030c`).

## Cause

`flat_card_reason` (#795) required one extent axis of the stored positions to be
exactly zero. The C1C cards are axis-aligned; these six are stored **tilted**, so
no axis is zero and the rule never fired — in `build_scenery`, whose selection
already ran with `hide_flat_colour_cards`.

## Fix

The planarity test is now geometric (`is_planar`): a triangle is planar, a larger
polygon is planar when its corners lie in one plane. Still keyed on the stored
attributes (one polygon, every material group untextured), not on names. The six
cones are no longer drawn; the shipwreck's hull parts, terrain and the other
scenery are unchanged (the census otherwise differs by nothing). The objective's
`TRAVELERS [player, APPROACHING, shipwreck, 200.0, 1]` reads the node anchor, not
any mesh, and is untouched.

## Unknown

* The cones' original role (a trigger/sensor volume, a flyover marker, or art
  drawn in a render class this reader does not decode) is **unmeasured**; the
  hiding is provisional like #795's.
* The cone cards have no collider already (colliders follow the drawn set; the
  scenery has none, #1752).
* One other 1-triangle, 0.5 × 5.6 × 0.9 m flat part at (-6578, 350, -7654) is
  still drawn (the new rule does not hide it; not investigated here).
* No original-game comparison frame exists; owner-only.
