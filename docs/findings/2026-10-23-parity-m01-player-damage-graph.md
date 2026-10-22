# PARITY-M01-PLAYER-DAMAGE-GRAPH: the player's region nodes, and what stays unmeasured

Date: 2026-10-23. Task: #1699. Capabilities used: `retail`.

## What changed

`crates/cs_app/src/mission_session/player_victim.rs`: the player's damage graph
(`damage-graph.m01-player`) now carries, beside the designed `hull` node, the
four region nodes `leftwingdamage`, `rightwingdamage`, `nosedamage` and
`taildamage`. The names are the ones measured in `ZBD/planes.zbd`
(`2026-10-07-f29-d1-airframe-damage-vocabulary.md`). `taildamage` is an
`Engine` node because the `player_pfighter` record names the tail part
`engine`; the other three are `InternalStructure`. The C1 pass therefore
publishes `Intact` for the player's regions at spawn, and a hit routed to a
wing node makes the C1 record `Damaged`, which shows the matching
`wing_flare` quad through `drive_airframe_damage_visuals`.

## Designed, not measured

Per-region integrity is `DESIGNED_REGION_INTEGRITY` (a quarter of the designed
hull pool, 300), filed under claim `m01-player-victim.designed-region-integrity`
with `Provenance::designed`. The vehicle record's `destroyable_parts` carry no
recovered integrity, so the explicit unknown
`m01-player-victim.region-integrity-unmeasured` (`PlayerVictim::region_unmeasured`)
stands beside it. Unmeasured and not invented:

- the original's per-part integrity and the damage level (green/yellow/red)
  thresholds;
- whether `critical` on a `destroyable_part` means the actor dies when it is
  destroyed — the region nodes are therefore **not** lethal;
- the effect of a destroyed `taildamage` engine node on propulsion (the node is
  an `Engine`, so the resolver's propulsion gate applies if a hit reaches it).

## Hit routing: not yet decoded

The region nodes are reached only by a hit event that names them. The player's
swept hull box still routes every round to `hull`: which hit box belongs to
which region is an authored `object3d` transform that no reader decodes
(see `part_sweep_geometry`). Routing of in-flight rounds into the wing nodes is
follow-up work.
