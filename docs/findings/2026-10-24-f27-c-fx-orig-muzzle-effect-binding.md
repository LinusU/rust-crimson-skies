# F27-C-FX-ORIG: the original's muzzle-effect binding

Date: 2026-10-24. Task: #1684. Follows
`docs/findings/2026-10-19-f27-c-fx-render-muzzle-flash.md` (#1595), which drew a
designed stand-in because the id to asset mapping was unrecovered.
Capability used: `retail` (read-only `$CS_GAME_DIR`). Provenance of every claim
below: `static` data read from the installation; nothing here is
`verified_original` (no original run was compared).

## What was measured

The binding is **by name, through the weapon row**, and it needs no
`hardpoint_equipment` catalog in the original:

1. `ZBD/zrdr.zbd` member `weapons.zrd`, `BALLISTICS` > `wep_NN` > `FIRE` >
   `ANIMATION` names an animation. 35 of the weapon records have one:
   - guns: `muzzle_burst_slug` (14 rows: `wep_00..03`, `wep_30`, `wep_40`,
     `wep_50`, `wep_60`, `wep_70`, `wep_130..170`), `muzzle_burst_dum`
     (`wep_31/41/51/61/71`), `muzzle_burst_ap` (`wep_32/42/52/62/72`),
     `muzzle_burst_mag` (`wep_33/43/53/63/73`);
   - `muzzle_burst2` (`wep_23`, `wep_29`); other names (`generate_smokescreen`,
     `call_small_flash`, `small_fireball`, `large_fireball`) belong to other
     weapon kinds and are not read here.
   - The M01 selected gun `wep_50` (`50slug`, `SELECTED_GUN_RECORD`) binds
     `muzzle_burst_slug`.
2. Each `muzzle_burst*` name is defined **exactly once** as an
   `ANIMATION_DEFINITION` `NAME` in `zrdr.zbd` member `muzzle_burst.zrd`
   (31 of 31 bindings resolve). The definition's `NAME` is also the name of a
   scene node: all eight world `gamez.zbd` containers (`C1`, `C1B`, `C1C`, `C2`,
   `C2B`, `C3`, `C4`, `C5`) hold a root node `muzzle_burst_slug` / `_dum` /
   `_ap` / `_mag` with two children, `mb1` and `mb_spinflame`.
3. **Drawable.** Per gun kind, two billboard meshes, both textured materials
   (alpha 255, material flags 17):
   - `mb1`: one quad, 0.34 wide (x = ±0.17, y = 0), z from -0.195 to 0.4, texture
     `{slug,dum,ap,mag}_muzzle1.tif`;
   - `mb_spinflame`: three quads (12 vertices) 60 degrees apart about the z axis,
     z up to 0.4 (slug), texture `{slug,dum,ap,mag}_muzzle2.tif`.
4. **Size / scale.** The definition sets `OBJECT_SCALE_STATE` `1.0 1.0 1.0` on the
   root node: the mesh units above are the drawn size (units: the game's world
   unit; metres are assumed by every other table here, not measured).
5. **Duration.** The root node is made active, then inactive again at
   `START_TIME EVENT_OFFSET 0.001`: 0.001 (seconds, unit assumed) after the call,
   i.e. shorter than one tick at any rate this project runs. The visual is
   therefore a single-frame flash, not a 6-tick one.
6. **Orientation.** `mb_spinflame` is rolled about z to one of three angles chosen
   by `RANDOM_WEIGHT` 0.333 each: `0 0 30`, `0 0 80`, `0 0 140`. The shared
   `muzzleburst_effects` animation is called `AT_NODE <burst> 0 -0.2 -1`.
   The geometry extends along +z of the node; which world direction the node's
   z maps to on a gun mount is not measured here.
7. **Companion effects** in the shared `muzzleburst_effects` animation (the
   definition named `muzzle_burst`, `ANIMATION_NAME muzzleburst_effects`): a smoke
   `PUFFER_STATE muzzlepuffer` (interval 0.05, `LOCAL_VELOCITY 0 0 -20`, size
   0.3-0.6, lifetime 0.1-0.2, growth 1.25, textures `smoke101..103`, switched off
   0.3 later) and six `LIGHT_STATE` events (`muzzle_lt`, `bigmuzzle_lt`; colour
   about 0.88-0.95 / 0.78-0.88 / 0.36-0.8; ranges from 1.0-2.0 up to 13.5-21.25 for
   the first-person pair), off after 0.0001. It also calls `gunshell` at the node.
   Sequence selection depends on `PLAYER_RANGE 30` and `PLAYER_1ST_PERSON`.

The retail tests are `accept_f27_c_fx_orig_*` in
`crates/cs_formats/tests/zbd/accept_f27_c_fx_orig.rs` (ignored without
`CS_GAME_DIR`, failing loudly when it is unset).

## Not measured, still unknown

- **Engine-side consumer.** The engine image was not disassembled here; that
  weapon fire calls the `FIRE` animation by this name is read from the data, not
  proven in code. Resolving task: filed as a follow-up.
- Which axis of the gun mount the node's +z is, and the handedness of the
  `-0.2 -1` call offset. Affects flash placement.
- Blend/transparency behaviour of material flags 17 and what the texture
  contains (texture decode and a runtime texture consumer are separate work).
- The unit of `EVENT_OFFSET` (taken as seconds) and `RANGE` of the lights.
- `muzzle_burst2` (`wep_23`, `wep_29`) scale states 4/3.5/2.5 are recorded in the
  data; its node mesh is in the same containers but is not asserted here.

## Production code

`resolve_muzzle_drawable` is **unchanged** and still returns the designed
stand-in: replacing it needs the texture decode, the mount orientation and the
single-frame lifetime above, none of which this measurement-only task can bind
without guessing. The stand-in's claim `f27-c-fx-render.designed-muzzle-flash`
therefore stays open; its note should now point to this finding for the measured
target (`muzzle_burst_slug` for `wep_50`).

## Not claimed

Checked, not `verified_original`; no human review; no screenshot comparison.
