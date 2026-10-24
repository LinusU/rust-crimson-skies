# M01 / C3: the `do_approach1..6` drop-off nodes compose a position (#2094)

Resolves the first row of section 4 of `2026-10-24-m01-c3-win-route-census.md`.

## What `treasure_approach` is

The six `do_approach1..6` landing nodes hang, in the `zbd/c3` world container's
scene graph, from the ancestor directly under the world root named
`treasure_approach`. `2026-10-18-m01-c3-world-residuals.md` (section A) lists it
among the 47 grouping records of the flat world import: a positioned `object3d`
record (it is not a bare marker), with children. No world actor binds that name,
so the observation never reports a row for it.

## Rule (static, no guess)

The container frame is the composition's frame (`2026-10-16-m01-play-triggers-anims-land.md`
section 3, fact 1). For an anchor that is a **bound world actor** the container's
position is not where the actor is (the container's zeppelin sits at the origin),
so the writer joins on the live actor row plus the container offset, as before.
For an anchor that is **no bound actor and not the player**, nothing moves it, and
the container's own position for the anchor is its position. `LandingNodeEntry`
therefore carries `anchor_rest` (the anchor's container world position) and the
observation writer uses it only for such a static anchor: the node composes at
`anchor_rest + offset`, which is the node's container world position.

An actor anchor whose row is absent (out of play, or no lifecycle answer) still
composes **no** node: the fallback never applies to a name some bound actor spells.

## Evidence

`accept_m01_do_approach_position_nodes_sit_at_the_static_anchor` (ignored without
`CS_GAME_DIR`) asserts, over the composed M01, that each of `do_approach1..6`
has a node entry anchored on `treasure_approach`, is observed in play at
`anchor_rest + offset`, and that `pz_manual_land` still sits at the live
`piratezep` plus its offset. The census test
`..._the_composed_mission_carries_the_route_inputs` now asserts the six are
positioned.

## Not established

Nothing here says what landing at those nodes does afterwards (`texdrop`, callback
951): still the census's open unknown. The position is the container's static
position; whether the original moves `treasure_approach` at run time is not
measured here.
