# M01 HUD art: the original gauge dials are mesh nodes in `planes.zbd`

Rally #1726 `PARITY2-M01-HUD-ART`. Task: draw the original gauge dials,
damage silhouette, reticle and compass strip instead of the text-circle
approximation from #1641. This records what the retail data measures so the
implementation does not re-derive it.

Status: **measured, not yet implemented** (checkpointed). All numbers below
come from the owner's retail installation at `$CS_GAME_DIR`; nothing here is
`verified_original` behaviour.

## Where the original HUD art lives

The analog dials the owner captures show are **not flat sprites**: each dial
is a subtree of `Object3d` nodes with mesh bindings inside the player
airframe's scene in `zbd/planes.zbd`. The M01 player is the `player` root
(stored slot 1418) → `player_pfighter` (44) → `cockpit1` (49) → `gauges`
(56). A second copy of the same subtree hangs under the generic
`piratefighter` root (`cockpit1` slot 243) — the mission code must use the
`player` tree, which alone carries the cockpit interior.

### `gauges` subtree (slot 56), stored transforms are cockpit-dash space

| slot | name | mesh | material textures | role |
|---|---|---|---|---|
| 57 | `horiz` | #37 | `horizonindicator.tif` | attitude frame (not in `instruments.zrd`; cockpit view only) |
| 61 | `pfhorizon` | #0 | none | attitude ball child of `horiz`/`horiz` |
| 58/64 | `comp`/`comp` | — | — | compass group |
| 65 | `compass` | #1 | `compasstxt.tif` ×8, `compassticks2.tif` | heading tape |
| 66 | `g730` | #2 | `compasstxt.tif`, `compassticks2.tif`, `bezel.tif` | compass bezel/window |
| 59 | `altimeter` | — | — | group |
| 69 | `hundreds` | #3 | `needle.tif` | alt needle, one rev / 1000 ft |
| 70 | `thousands` | #4 | `needle.tif` | alt needle, one rev / 10 000 ft |
| 71 | `lowalt_on` | #5 | `lowalt.tif`, `redhilite.tif` ×2 | LOWALT flag |
| 73 | `g784` | #6 | `altimeter.tif` | dial face |
| 63 | `speedometer` | — | — | group |
| 75 | `speed` | #7 | `needle.tif` | speed needle |
| 76 | `stallwarning_on` | #8 | `redhilite.tif` ×2, `stall.tif` | STALL flag |
| 78 | `g784` | #9 | `speedometer.tif` | dial face |
| 68 | `damageindicator` | — | — | group |
| 74 | `g843` | #38 | `de_damage.tif` | silhouette face (per-airframe: `<airframe>_damage.tif`) |
| 79–82 | `rightwingdamage`…`nosedamage` | #39–#42 | `greenhilite.tif`, `grn_hatchptrn.tif` | 4 damage zones |
| 83 | `missilegauge` | — | — | group |
| 87 | `4char_ammo` | #10 | `one.tif` ×4 | 4 ammo digits (digit-swap quads) |
| 88–97 | `mgindicator0..7` | #11–#20 | `greenhilite.tif`+`greenindicator.tif` | 8 rocket pips |
| 92 | `mgarrow` | #15 | `smallneedle.tif` | ordnance-type arrow |
| 93 | `6char_type` | #16 | `A.tif` ×6 | 6 name glyphs (letter-swap quads) |
| 98 | `g819` | #21 | `missilegauge.tif` | dial face |
| 84 | `gungauge` | — | — | same pattern: `4char_ammo` #22, `ggindicator0..3` #23–#26, `ggarrow` #27, `6char_type` #28, `g815` #29 (`gungauge.tif`) |
| 85 | `nitrogauge` | — | — | `nitro_boost` #30 (`needle.tif`), `nitro_backplate` #31 (`brushedmetal.tif`,`nitrogauge.tif`), `nitro_charge` #32 (`needle.tif`) |

Sibling cockpit furniture under `cockpit1` not listed in `instruments.zrd`
(and so not part of the HUD overlay): `lailer3`/`railer3` aileron indicators,
`pcdp6`/`pcdp4`, `g743`/`g748` bullet decals.

### Digit/letter quads

`4char_ammo` binds `one.tif` on all four groups and `6char_type` binds
`A.tif` on all six: the original swaps each quad's texture to the digit or
letter it needs. `ZBD/C3/texture.zbd` should be checked for the glyph set
(`zero.tif`…`nine.tif`, `A.tif`…); if absent the count can be kept as text
over the gauge window, but the glyphs should be preferred.

## Placement: `ZBD/zrdr.zbd` → `instruments.zrd`

The `.zrd` (read through `cs_content::stunts::decode_zrd`) gives each
instrument's translation in the HUD instrument space the second camera pass
draws:

| node | x | y | z |
|---|---|---|---|
| `speedometer` | 9.0 | −4.05 | −17.5 |
| `altimeter` | −9.0 | −4.05 | −17.5 |
| `comp` | 0.0 | 7.0 | −17.5 |
| `damageindicator` | −9.0 | −6.0 | −17.5 |
| `missilegauge` | −9.0 | −2.1 | −17.5 |
| `gungauge` | 9.0 | −2.1 | −17.5 |
| `nitrogauge` | 11.5 | −10.5 | −23.5 |

`comp` stores a second triple `21.0, 0.0, 0.0` (likely a scale/other
placement — unmeasured).

Stored dash transforms differ (e.g. altimeter `local_t=(2.0,−4.93,−10.32)`):
the cockpit pass uses the stored values, the HUD pass uses this table. Parts
must spawn at `instruments.zrd` translation plus their transform **relative
to the instrument node**.

The instrument space fits a camera at the origin looking down −Z with
half-extents ≈ **10.0 wide × 7.5 high at |z| = 17.5** (4:3), i.e. a vertical
fov of `2·atan(7.5/17.5) ≈ 46.4°`: `comp` at y=7 lands at the top edge and
the dial groups anchor in the bottom corners exactly as the owner captures
show. The fit itself is designed; the placements are measured.
`nitrogauge` at z=−23.5 projects to the bottom-right corner — the original
shows only its top sliver there (capture: ~120×7 px at x≈900–1024,
y≈630–637).

**Dial radius fit (2026-10-20, second session).** In the owner's 1024×768
frame the round gauges measure ~92–95 px across: the left stack sits at
x≈9–104 (centre −8.9 in HUD units) with rows y≈447–537 / 544–636 / 642–735,
whose centres land on `missilegauge` −2.1, `altimeter` −4.05 and
`damageindicator` −6.0 to within 0.1 unit, and the right column mirrors it
at x≈921–1016 (+8.9). That is a dial radius of ≈**0.9** HUD units, not the
first checkpoint's ≈3 guess — the speedometer `g784` face's stored radius
measures ≈1.0 in instrument space, so the drawn scale is ≈0.9. The compass
strip measures 140×20 px centred at top (x 442–582, y 19–39 ≈ 2.7 units
wide at the tape's own scale); `comp`'s second stored triple `(21, 0, 0)`
stays unmeasured — it is not a per-instrument scale (the comp subtree is
authored in the same unit space as the dials).

## Measured engine constants (already committed)

`data/owner-static/engine-facts.toml` (static, VA-cited):

- HUD airspeed `mph = m/s × 2.2369363` (VA `0x6076e4`).
- Speed-needle rate `−0.02811017 rad per m/s` (VA `0x6076e8`) = `2π/500 mph` —
  one needle revolution per 500 mph, negative = the stored winding.
- HUD altitude `ft = m × 3.2808399`.
- Camera looks along −Z, +Y up, right-handed — matches the instrument space
  above.

The altimeter `hundreds`/`thousands` needles share `needle.tif`; treating
them as the same family (`−2π` per 1000 ft / per 10 000 ft) is consistent
with the 0–9 `altimeter.tif` face but the exact per-needle scale is
**designed**, not engine-measured.

## HUD colours: `ZBD/zrdr.zbd` → `hud_v2.zrd` (`HUD_COMMON`)

- `LIGHT_HUD_NORM` (14,196,8), `LIGHT_HUD_BRIGHT` (24,240,16)
- `DARK_HUD_NORM` (14,76,8), `DARK_HUD_BRIGHT` (24,120,16)
- `COLOR_RED` (200,20,20), `COLOR_BLUE` (40,40,240)

`SW_GAUGES` names font `hudSWGauges` and holds normalised first-/third-
person positions (AIR_SPEED `(0.1,0.8)`/`(0.1,0.2)`, ALT/GUNS/MISSILES/
HEALTH/NITRO stepping 0.02) — the software-renderer fallback, not needed
once the mesh gauges draw, but the source of the HUD green if UI text
remains.

## Damage zones

Per-airframe zone state variants are enumerated in `vehicle.zrd`
(`nosedamage`, `leftwingdamage`, `rightwingdamage`, `taildamage`; states
`*_damage_green|yellow|red`). The zone meshes bind
`greenhilite.tif`+`grn_hatchptrn.tif` intact; `ZBD/C3/texture.zbd` ships the
matching `yellowhilite`/`yel_hatchptrn`, `orangehilite`/`orng_hatchptrn`,
`redhilite`/`red_hatchptrn`, `bluehilite` set — the damaged states are a
material swap on the zone mesh, not a tint of the silhouette. Which zone
threshold selects yellow vs red is unmeasured; `cs_sim`'s
`PartState::{Intact,Damaged,Destroyed}` maps naturally
green→yellow/orange→red (designed mapping).

## UI textures in `ZBD/rimage.zbd`

- `impact_point` 32×32 (alpha): the centre reticle — ring with cross ticks. Identified by
  visual match against the owner's original M01 flight frames (#2016); the engine consumer
  was not traced. Correction: an earlier version of this note named `rcircle` here from the
  texture name alone.
- `rcircle`/`rcircle_y` 32×36 (alpha plane): a hand-drawn felt-marker circle. The owner
  recognises it from the briefing/debrief map callouts (owner observation, consumer not
  source-verified). **Not** the flight reticle; kept unused for the map screens.
- `alignment_point` 32×32: decodes fully black/transparent; role unknown.
- `arrow` 35×28, `darrow`/`uarrow` 36×43, `larrow`/`rarrow` 43×36:
  off-screen contact direction arrows (the `<`/`>`/`^`/`v` edge markers in
  the captures).
- `guage` 168×9: thin status bar (health bar?).
- `donut`/`donuts`/`donutx` 8×8; `1_circle`–`4_circle` 69×49;
  `5pointhud`/`5pointhudbrite` 463×6 font strips;
  `impact_point`, `alignment_point` 32×32; `frame2`, `miss_frame`.

## Suggested implementation shape (checkpoint note)

1. `mission_session/hud_art.rs`: at compose time (visuals branch of
   `add_composition`), build `MissionHudArt` from
   `PlayerAirframeSource.sources()` — `aircraft_graph` over `planes.zbd`,
   `gauges` subtree of `player`/`player_pfighter`, same
   `render_of`/`WorldMeshes::insert_render_mesh`/`TextureBinder::parts`
   pipeline as `build_aircraft_visual`, but keeping each part's transform
   **relative to its instrument node** and reading `instruments.zrd`
   placements from `ZBD/zrdr.zbd`.
2. Spawn under a second `Camera3d` (`RenderLayers` layer, `order: 1`, no
   clear) whose viewport is the 4:3 `safe_area` rectangle, fov per the fit
   above.
3. Drive: `speed` rotation `−0.02811017·v` rad (m/s); `compass` tape
   rotation by heading; `hundreds`/`thousands` by ft; `mgindicatorN`/
   `ggindicatorN` visibility by rocket/round count vs per-pip unit
   (unmeasured — label designed); zone material swap by `PartState`;
   `lowalt_on`/`stallwarning_on` flags (thresholds unmeasured).
4. Reticle: `impact_point` texture as a centred UI `ImageNode` (decode through
   `read_zbd_textures` + the front-end `DecodedImage`→`Image` path).
5. Values: reuse `cockpit_hud::read_values` sources; it needs per-zone
   states (not just `damage_fraction`) — `airframe_zones` already yields
   them.

Reference captures used: task-history `original.png` chase/damaged/cockpit
screenshots (kept under `private/hud-art/`, not committed).
