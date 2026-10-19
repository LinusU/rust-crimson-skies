# PARITY-M01-PLAYER-VIEW-C2: wing fire and smoke plume source data

Task #1683. Reference: owner captures `private/ref-drydock/m01/orig70.png`
(damaged: orange/yellow fire trail off the **left** wingtip, dark grey smoke
puffs, red/yellow damage gauge) and `orig25.png` (intact: **no** wingtip
flares, **no** damage patches, all-green gauge). Captures never committed.

## Measured — `ZBD/planes.zbd`, player airframe `player_pfighter`

The effect's drawn source is authored **geometry**, not an emitter record:

| Node | Slot | Mesh | Texture (material) | Position | Polys |
| --- | --- | --- | --- | --- | --- |
| `wing_flare1` | 208 | 105 | `oil_liteflare.tif` (mat 73, flags 0x11) | `[-5.3746, -0.8091, 2.2797]` | 1 quad, ±0.5625 m |
| `wing_flare2` | 209 | 106 | `oil_liteflare.tif` (mat 73) | `[+5.3200, -0.8120, 2.2009]` | 1 quad, ±0.5625 m |

Both under `dontmove` (207) — outside the `healthy` intact subtree, so the
production selection draws them only via a pin, same as `staticprop1`/`prop1`.

Other measured damage-presentation data (same container):

- `healthy > nearest > player_damage_on` parents ten `pdp*` patch meshes
  (`damage1.tif`/`damage2.tif`/`dev_*` decals). `player_damage_off` markers
  exist on other airframes. Per-patch region mapping: **unknown**.
- `destroyed` subtree: `piece1`–`piece4` wreck meshes (101–104).
- `damageindicator` cockpit gauge: four region meshes (`leftwingdamage` etc.,
  meshes 39–42) whose materials are texture **cycles**: `greenhilite` cycles
  `[green, yellow, orange, red]` (texture ids 10/266/294/5), `grn_hatchptrn`
  cycles `[grn, yel, orng, red]` (35/295/296/297), `greenindicator` cycles
  `[green, yellow, red]` (11/267/268). The original per-region gauge colour
  rule is therefore **a damage-level-driven texture cycle**, not a tint.

## Unknown — recorded, not guessed

- The trailing fire/smoke **particles** in orig70: no emitter, rate, lifetime,
  size, colour table or trail record exists in the container's node/mesh/
  material data. If engine-side, it needs engine-image measurement (a
  `data/owner-static/engine-facts.toml` claim) — none recovered yet.
- The exact damage level at which the original first shows a flare (the
  production gate is the task's own rule: `Damaged`/`Destroyed`).
- `wing_flare1`→left/`wing_flare2`→right rests on measured translations
  (x ∓5.4 m), not on an authored semantic tag.
- The cycle-frame index the gauge selects per damage level (4 frames vs the
  resolver's 3 states), and the per-region `pdp*` patch mapping.
- The M01 player's damage **graph**: the actor has no region nodes in the
  session resolver (C1 finding), so today its regions publish `Unknown` and
  the flares never show — matching orig25's intact presentation.

## Implementation this finding backs

`WingFlare { region }` + `update_wing_flares` in `airframe_damage.rs`:
each quad carries its region, the pass declares the region binding once the
session marker exists, and the quad shows exactly while the published
`PartState` is `Damaged`/`Destroyed`. `aircraft_flare_nodes` pins slots
208/209 (`wing_flare1`/`wing_flare2` name-checked) into the draw set.
