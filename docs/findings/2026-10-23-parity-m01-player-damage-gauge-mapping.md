# PARITY-M01-PLAYER-DAMAGE-GAUGE-A: player damage ladder and gauge cycle rule

Task #1942. Sources: `ZBD/zrdr.zbd` (`vehicle.zrd` record `pdevastator`, whose
`nodename` is `player_pfighter`; `player.zrd` animation definitions) and
`ZBD/C3/cam_anim.zbd` (compiled records). Read with
`cs_formats::zbd::{read_vehicle_records, read_animation_definitions}` and the
new `EffectAnimation::cycle_textures`. Provenance: `static` (retail files
read; never `verified_original`). Tests:
`accept_parity_m01_player_damage_gauge_a_*` in `crates/cs_formats/tests/zbd/`.

## Correction to earlier findings

The `<part>_damage_<green|yellow|red>` names are **not** per-level fields that
*value* to `pdpanelN` (the C2 finding's reading). They are **entries of each
part's `injure_anims` ladder** (threshold, animation), interleaved with the
`pdpanelN` entries. And `pdpanelN` anims do **not** touch the gauge: none holds
a texture event.

## Measured: the ladder per part (`pdevastator.destroyable_parts`)

Thresholds descend. `<p>` is the part name (`nose`, `tail`, `leftwing`,
`rightwing`); every part carries `0.99 <p>_damage_effects`,
`0.72 <p>_damage_green`, `0.46 <p>_damage_yellow`, `0.2 <p>_damage_red`.
The `pdpanelN` entries between/after them:

| part | pdpanel entries (threshold → anim) |
| --- | --- |
| nose | 0.15 → `pdpanel7` |
| tail | 0.15 → `pdpanel8` |
| leftwing | 0.5 → `pdpanel5`, 0.3 → `pdpanel4`, 0.15 → `pdpanel3` |
| rightwing | 0.4 → `pdpanel6`, 0.3 → `pdpanel1`, 0.15 → `pdpanel2` |

Vehicle level: `0.1 player_smoketrail`, `0.85 player_fuelleak` (see the
DMG-FX-A finding).

## Measured: what each animation does

- `<p>_damage_green|yellow|red` (object `<p>`): one
  `OBJECT_CYCLE_TEXTURE (NAME (<p>damage) RESET (n))` with **n = 1 green,
  2 yellow, 3 red**, identical for all four parts (`nosedamage`, `taildamage`,
  `leftwingdamage`, `rightwingdamage` are the cockpit zone nodes). The effect
  calls (`random_gun_impact`) vary by part/level and are not gauge data.
- `pdpanel1`–`pdpanel8` (object `player_pfighter`): `OBJECT_ACTIVE_STATE` of
  node `pdpN` plus fireball/flake/firetrail calls at `pdpN` (panels 4 and 6
  also `pcdpN` with a `PLAYER_1ST_PERSON` branch). **No cycle-texture event**:
  they are body-damage patch/fire effects, not the cockpit gauge.
- `<p>_got_hit`: toggles `OBJECT_ACTIVE_STATE` of `<p>damage` INACTIVE, then
  ACTIVE after 0.2 s, `LOOP_COUNT 8` — a hit **blink** of the zone.
- Compiled `cam_anim.zbd` repeats the value: tag `0x111`, size `0x14`, the
  16-bit word at event `+0x12` equals `RESET`. `<p>_damage_yellow`/`red` hold
  one such event (2, 3); `<p>_damage_green` holds an extra leading sequence with
  an event of value **0**, then the event with 1.

## Gauge materials

The four zone meshes bind `greenhilite` and `grn_hatchptrn` (4-frame cycles
`[green, yellow, orange, red]`). `greenindicator` (3 frames) is bound by the
`mgindicatorN`/`ggindicatorN` ammo pips (HUD-art finding), **not** by the
damage zones, so it is irrelevant to this gauge.

## Still unknown (with evidence)

1. **What `RESET` indexes.** 0-based gives green anim → frame 1 (yellow),
   yellow → orange, red → red: a clean progression across the four frames and
   consistent with the leading value-0 event in `*_damage_green` (frame 0 =
   intact green). 1-based gives green/yellow/orange and never red. The text
   does not say; no engine-image read was done here. Resolve before driving
   the gauge (task #1943): an engine-image fact or an owner capture of a
   damaged cockpit.
2. **Threshold direction/unit** (remaining health fraction vs damage
   fraction) is still inferred from descending order, as in DMG-FX-A.
3. The `25 25` numbers on each part, and what feeds a part's 0..1 health.
4. Whether the compiled `+0x0c` word of the cycle event names the node by
   index (it is `1` in every record read); node resolution is unmeasured.
5. The orange `hilite` frame has no named ladder entry: with the 0-based
   reading it belongs to the 0.46 level, so `hud_art.rs::zone_material`'s
   current Damaged→yellow, Destroyed→red mapping skips it.
