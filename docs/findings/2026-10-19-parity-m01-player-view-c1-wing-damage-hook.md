# PARITY-M01-PLAYER-VIEW-C1: the wing damage presentation hook and what is unmeasured

Date: 2026-10-19. Task: #1682 (slice C1 of #1668, under #1644). Visual target:
the owner capture `orig70.png` (+70 s) in the gitignored
`private/ref-drydock/m01/`; never committed. Capabilities used: `retail`.

## What changed

`crates/cs_app/src/airframe_damage.rs`: `AirframeDamageRegion` (the four
measured names of `2026-10-07-f29-d1-airframe-damage-vocabulary.md`),
`RegionDamageBinding` (declared by a spawn path: region → damage node),
`RegionDamagePresentation` (the record), `apply_region_damage_state` (a
convergent pass that copies `DamageResolver::part_state` verbatim) and
`release_region_damage` (repair / reload / scene-release teardown). Nothing is
rendered. Wiring edit: `pub mod airframe_damage;` in `cs_app/src/lib.rs`.

## Measured: the player's region → node mapping

Test `accept_parity_m01_player_view_c1_retail_m01_player_graph_has_no_region_nodes`
(ignored, `CS_GAME_DIR`) composes M01 headlessly and reads the session
`SessionDamageResolver` for `MissionWeapons::player()`: **no graph node of the
player is named `leftwingdamage`, `rightwingdamage`, `nosedamage` or
`taildamage`.** The only `register_actor` callers (`mission_actors.rs`) register
scripted aircraft, world-actor parts and turrets, not the player's airframe.

So the mapping is **unknown / absent**, not guessed: no spawn path declares a
`RegionDamageBinding` for the player, and the pass publishes nothing for it.
A binding for a node the resolver does not describe is published as
`Unknown` with a refusal line. `zone_id` is 255 on all measured region records,
so it cannot supply the mapping either.

Resolving task needed: a player-airframe damage graph (nodes, integrity,
hit routing) is a separate, unmeasured piece; integrity numbers are not in
the data (F29-D.1: no recoverable integrity or armour values).

## Unknown, with `orig70.png` as the target

What the capture shows: orange/yellow fire trail from the left wing tip, dark
grey smoke behind and below, the bottom-left damage gauge drawn red with a
yellow fuselage bar, and the prop blur disc. Unmeasured:

- the damage state / threshold at which fire and smoke start (the resolver's
  `Damaged` is "some integrity absorbed"; whether the original uses it is unknown);
- particle size, colour, rate, lifetime and trail length;
- the emitter anchors (`wing_flare1/2` exist under the player airframe, unmeasured
  as emitters) and whether it is sprites or geometry;
- the per-region gauge colour rule.

## HUD gap

`cockpit_hud.rs` `damage_tint` is a designed 3-bar tint (damaged 0.5, destroyed
1.0, green → red), not the original's per-region colours (red wings and tail,
yellow fuselage). The original rule is unmeasured; the gauge is not changed here.
