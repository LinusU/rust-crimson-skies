# PARITY-M01-PLAYER-VIEW-C2: wing fire and smoke plume source data

Task #1683. Reference: owner captures `private/ref-drydock/m01/orig70.png`
(damaged: orange/yellow fire trail off the **left** wingtip, dark grey smoke
puffs trailing behind/below, red/yellow damage gauge) and `orig25.png`
(intact: **no** wingtip flares, **no** damage patches, all-green gauge).
Captures never committed.

## Measured — `ZBD/planes.zbd`, player airframe `player_pfighter`

The effect's drawn source is authored **geometry**, not an emitter record:

| Node | Slot | Mesh | Texture (material) | Position | Polys |
| --- | --- | --- | --- | --- | --- |
| `wing_flare1` | 208 | 105 | `oil_liteflare.tif` (mat 73, flags 0x11) | `[-5.3746, -0.8091, 2.2797]` | 1 quad, ±0.5625 m |
| `wing_flare2` | 209 | 106 | `oil_liteflare.tif` (mat 73) | `[+5.3200, -0.8120, 2.2009]` | 1 quad, ±0.5625 m |

Both under `dontmove` (207) — outside the `healthy` intact subtree, so the
production selection draws them only via a pin, same as `staticprop1`/`prop1`.

**Handedness is now measured, not inferred:** the scene's own control
surfaces set the x convention — `l_aileron1`/`l_aileron2`/`l_elevator1` sit at
x < 0 and `r_aileron*`/`r_elevator1` at x > 0 in the composed M01 world
(airframe x ∓ = port/starboard). `wing_flare1` at −5.37 is the **port**
quad, `wing_flare2` at +5.32 the **starboard** one.

Other measured damage-presentation data (same container):

- `healthy > nearest > player_damage_on` parents ten `pdp*` patch meshes
  (`damage1.tif`/`damage2.tif`/`dev_*` decals). `player_damage_off` markers
  exist on other airframes.
- `destroyed` subtree: `piece1`–`piece4` wreck meshes (101–104).
- `damageindicator` cockpit gauge: four region meshes (`leftwingdamage` etc.,
  meshes 39–42) whose materials are texture **cycles**: `greenhilite` cycles
  `[green, yellow, orange, red]` (texture ids 10/266/294/5), `grn_hatchptrn`
  cycles `[grn, yel, orng, red]` (35/295/296/297), `greenindicator` cycles
  `[green, yellow, red]` (11/267/268). The original per-region gauge colour
  rule is therefore **a damage-level-driven texture cycle**, not a tint.

## Measured — the anim/effect chain behind the plume

Member names below are `cs-inspect` strings of the retail containers
(offsets are bytes into the container, not an asset format).

`ZBD/zrdr.zbd`, record `player_pfighter` (the vehicle record):

- top-level `injure_anims` = `player_smoketrail`, `player_fuelleak`;
  also `spin_props_anim` = `spinprops`, `stop_props_anim` = `stopprops`,
  `start_anims` = `wing_lights_blink`, `bullethole_anims` = `bullet1`–`bullet5`,
  `reset_bulletholes`, and a `collision` part;
- `destroyable_parts` = `nose`, `tail` (engine), `leftwing`, `rightwing`,
  each `critical`, each carrying `got_hit_anim` = `<part>_got_hit`,
  `injure_anims` = `<part>_damage_effects`, and per-level fields
  `<part>_damage_<green|yellow|red>` whose values are `pdpanel*` gauge anims.
  String order pins `leftwing` cleanly — green→`pdpanel5`, yellow→`pdpanel4`,
  red→`pdpanel3` — while `nose`/`tail`/`rightwing` reference
  `pdpanel7`/`pdpanel8`/`pdpanel6,1,2` in an order that needs record-level
  decode before a level mapping is claimed.

`ZBD/C3/cam_anim.zbd`, anim records bound to `player_pfighter`:

| Record | Bound scene nodes | Effect played |
| --- | --- | --- |
| `player_smoketrail` (0x36254) | `wing_flare2`, `fire_lt`, `prop1` | `dense_firetrail` (`local_dft`) |
| `player_fuelleak` (0x35c48) | `wing_flare2`, `winglight2`, `pdp1`–`pdp3` | `fuel_trail` |
| `player_firetrail` (0x36b90) | `fire_lt`, `prop1` | `dense_firetrail` |
| `player_damage_trail` (0x370fc) | `fire_lt`, `prop1` | `short_firetrail` |
| `<part>_damage_effects` / `<part>_damage_<green|yellow|red>` | `<part>damage` nodes | `random_gun_impact` (green also `RESET_SEQUENCE`) |
| `pdpanel1`–`pdpanel8` | `pdp1`–`pdp8` etc. | per-panel effects |
| `random_remote_damage` | `l_aileron*`, `r_aileron*`, `*_elevator*` | `fireball`-family follows |

`ZBD/gamez.zbd` member table lists the effect names those records play —
`dense_firetrail`, `short_firetrail`, `large_firetrail`, `sputter_firetrail`,
`loop_short_firetrail`, `fuel_trail`, `injure_fireball`,
`small_injure_fireball`, `engine_start_smoke`, `fire_here`, `fire_here2`,
`partial_damage`, `partial_damage_obj`, `random_gun_impact` — and the texture
cycles the sprites draw: `smokepuffer` = `smoke101`–`smoke103`,
`firepuffer` = `fire_f01`–`fire_f06`, `fire1`/`fire2` = `fire101`–`fire112`.
`EFFECTS` records `fire1`/`fire2` pair those cycles with `fire1.flt`/`fire2.flt`.

So the original plume is an **anim-driven composite**: the wingtip flare quad
plus a `*_firetrail`/`fuel_trail` sprite trail anchored at `fire_lt`/`prop1`,
plus per-part `<part>_damage_*` anims and `pdpanel*` gauge anims — exactly
the composite orig70 shows.

## Still unknown — recorded, not guessed

- Numeric **emitter parameters** (rate, lifetime, size, colour table) of the
  trail/smoke effects: they live inside the binary anim/effect records and
  were not decoded — no runtime value was invented.
- Which trail plays at which damage level and on which side: `orig70`'s fire
  streams from the **port** tip (`wing_flare1`'s side) yet the trail records
  bind only `wing_flare2`; `fire_lt`'s own anchor node was not recovered
  (the name reads "fire light"/"fire left" — either reading is a guess), and
  whether these anims *move* nodes is undecoded. The production gate is
  therefore deliberately conservative: each quad answers to its own side's
  region.
- How `injure_anims` map onto `PartState` levels (top-level vs per-part), and
  the green/yellow/red thresholds the `<part>_damage_*` records key on.
- The cycle-frame index the gauge selects per damage level (4 frames vs the
  resolver's 3 states), and the per-region `pdp*` patch mapping.
- The M01 player's damage **graph**: the actor has no region nodes in the
  session resolver (C1 finding), so today its regions publish `Unknown` and
  the flares never show — matching orig25's intact presentation.

## Implementation this finding backs

`WingFlare { region }` + `update_wing_flares` + the scheduled
`drive_airframe_damage_visuals` in `airframe_damage.rs` (Update, after
`update_propeller_blur`): each pass declares the flare's region binding once
the session marker exists, publishes `RegionDamagePresentation` from the
session `DamageResolver`, and shows the quad exactly while the published
`PartState` is `Damaged`/`Destroyed`. `aircraft_flare_nodes` pins slots
208/209 (`wing_flare1`/`wing_flare2` name-checked) into the draw set. The
anim-driven sprite trail itself (`fire_lt` anchor, `dense_firetrail`
emitter, `smokepuffer`/`firepuffer` cycles) is follow-up work: its records
are measured above, its runtime is not yet implemented.

## Owner-command smoke — what it showed

`cs --cs-path $CS_GAME_DIR --mission M01 --smoke-seconds 45 --capture-dir
private/evidence/PARITY-M01-PLAYER-VIEW-C2` on this branch's reviewed commit:
all 14 checks pass (`passed: true`), 13 framebuffer PNGs and `report.json`
under the capture dir. The scripted smoke **never damages
the player** — the M01 player actor registers no damage graph, so its regions
publish `Unknown` and both flare quads stay hidden for the whole run. That is
the correct intact presentation (orig25.png shows no flares on a healthy
airframe); it also means the smoke cannot yet exercise the damaged path the
gate draws. The damage graph the smoke needs is follow-up
PARITY-M01-PLAYER-DAMAGE-GRAPH; the sprite trail is
PARITY-M01-PLAYER-DAMAGE-TRAIL; the gauge's texture-cycle rule is
PARITY-M01-PLAYER-DAMAGE-GAUGE.
