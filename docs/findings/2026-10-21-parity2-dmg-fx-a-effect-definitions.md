# PARITY2-DMG-FX-A: the fire, smoke and flak effect definitions, measured

Date: 2026-10-21. Task: #1805. Closes the open measurements of #1737
(`docs/findings/2026-10-19-parity-m01-player-view-c2-wing-fire-smoke-source.md`,
"Still unknown"). Capability used: `retail` (read-only `$CS_GAME_DIR`). No
original run happened; nothing here is `verified_original`, and nothing says
what the 2000 engine *does* with a parameter beyond what its spelling and the
cross-checks below show. No original bytes are committed: the numbers below are
small factual tables, each with the member and keyword it comes from.

## Code and tests

- `crates/cs_formats/src/zbd/reader_tokens.rs` (new): the compiled `.zrd`
  grammar as a typed tree (`ReaderNode`, `read_reader_node(s)`).
- `crates/cs_formats/src/zbd/effect_defs.rs` (new): `EffectAnimation` >
  `EffectSequence` > `EffectEvent` with `PufferState`, `LightState`,
  `CallAnimation` typed field by field, plus `read_vehicle_records`
  (`injure_anims`, `destroyable_parts`). Wiring: `zbd/mod.rs` (module
  declarations and re-exports).
- `crates/cs_formats/tests/zbd/accept_parity2_dmg_fx_a.rs` (registered in
  `tests/zbd/mod.rs`): five synthetic tests and seven retail tests
  (`#[ignore = "requires CS_GAME_DIR"]`), prefix `accept_parity2_dmg_fx_a_`.

## Where the definitions live (the correction to the earlier finding)

The earlier finding looked for "emitter records" in `cam_anim.zbd` and
`gamez.zbd` and found only names. The numbers are in **`ZBD/zrdr.zbd`**, the
shared reader archive: its 221 `.zrd` members decode, with no byte left over,
as a token stream (below), 162 of them as `ANIMATION_DEFINITIONS` (616
animation definitions). An effect is an `ANIMATION_DEFINITION`; a particle
emitter is a `PUFFER_STATE` event inside one of its sequences, carrying the
whole parameter set. `ZBD/C3/cam_anim.zbd` holds the *compiled* records of the
same definitions and repeats the floats (cross-check below);
`ZBD/C3/gamez.zbd` holds the **host nodes** the definitions are authored on.

### The token grammar (measured, all 221 members)

Every word is a little-endian `u32`: tag `1` = `i32`; tag `2` = `f32`; tag `3`
= string (`u32` length, bytes, no NUL); tag `4` = list (`u32` word `N`, then
`N - 1` children — the rule `dzones.zrd` found). A record is a flat run of
`"KEYWORD" (values...)` sibling pairs; `"ENDIF" ()` and `"ELSE" ()` are empty
lists. Numbers that print as integers are often floats (`25 25` in
`destroyable_parts` are tag 2).

### Host nodes in `ZBD/C3/gamez.zbd` (read with `read_gamez_nodes`)

The `NAME` of an effect definition is a node of the gamez node array:
`dense_firetrail` #88, `injure_fireball` #90, `small_injure_fireball` #92,
`sputter_firetrail` #78, `large_firetrail` #80, `fuel_trail` #82,
`short_firetrail` #84, `loop_short_firetrail` #86, `engine_start_smoke` #94,
`fire_here` #74, `fire_here2` #76, `partial_damage` #70,
`partial_damage_obj` #72, `flak` #245, `aaflak` #163, `flak_explosion` #128,
`flak_smoke` #124, `flak_fire` #126, `flak_trails` #432/#433,
`flak_trail1..5` #434.., `lg_fireball` #114, `sm_fireball` #120,
`lg_black_smoke_ball` #118, `planeflakes` #23 with `flake1..7`. All are
`Object3d` nodes with flags 40 (meshless hosts): the emitter has no geometry of
its own, it is a position. **`injure_fireball` has no `ANIMATION_DEFINITION`
named so in `zrdr.zbd`**: only the node (#90) and compiled-record strings
carry the name; the animations that draw the damage fireballs are
`small_fireball_follow`, `short_fire_follow` and `short_fireball_follow`, all
authored on the host `small_injure_fireball` (#92). That the node `injure_fireball` is
reached by some other path is not excluded and not measured.

## Which effect plays at which damage level (`vehicle.zrd`, `pdevastator`)

`vehicle.zrd` record `pdevastator` (`kind_of player_airplane`; the base record
`player_airplane` names `nodename player_pfighter` and carries no damage
fields). Eleven of the player airframe records carry the identical
vehicle-level list.

Vehicle-level `injure_anims` (threshold, animation):

| threshold | animation | made of (`player.zrd`, member 100) |
| ---: | --- | --- |
| 0.10 | `player_smoketrail` | `dense_firetrail` on `prop1` (`local_dft`, +0.1 s), sets `wing_flare2` INACTIVE; second sequence: `fire_lt` light flicker at `prop1`, 50 loops |
| 0.85 | `player_fuelleak` | one of three (weights 0.4 / 0.4 / rest): `3040slug_gunhit` at `pdp1`/`pdp2`/`pdp3` and `fuel_trail` `WITH_NODE` the same `pdpN`; the `pdp3` branch also sets `wing_flare2` and `winglight2` INACTIVE |

Not in the vehicle's list but defined beside them: `player_firetrail`
(`dense_firetrail` on `prop1` forever + the same light flicker, weight 0.2) and
`player_damage_trail` (`short_firetrail` on `prop1`, light weight 0.5). Who
calls them is not measured here.

`destroyable_parts` (`nose`, `tail`, `leftwing`, `rightwing`; each
`25 25 "critical"`, `tail` also `"engine"`; each `got_hit_anim <part>_got_hit`)
carry an `injure_anims` ladder, threshold descending:

| part | ladder (threshold animation) |
| --- | --- |
| `leftwing` | 0.99 `leftwing_damage_effects`, 0.72 `…_green`, **0.50 `pdpanel5`**, 0.46 `…_yellow`, **0.30 `pdpanel4`**, 0.20 `…_red`, **0.15 `pdpanel3`** |
| `rightwing` | 0.99 `…_effects`, 0.72 `…_green`, 0.46 `…_yellow`, **0.40 `pdpanel6`**, **0.30 `pdpanel1`**, 0.20 `…_red`, **0.15 `pdpanel2`** |
| `nose` | 0.99 `…_effects`, 0.72 `…_green`, 0.46 `…_yellow`, 0.20 `…_red`, **0.15 `pdpanel7`** |
| `tail` | 0.99 `…_effects`, 0.72 `…_green`, 0.46 `…_yellow`, 0.20 `…_red`, **0.15 `pdpanel8`** |

The `pdpanelN` anims (`player.zrd`) light `pdpN` (`OBJECT_ACTIVE_STATE`), throw
`gimmeflakes` debris at it and start the persistent fire:

| anim | node (planes.zbd, x) | effect |
| --- | --- | --- |
| `pdpanel1` | `pdp1` (+4.54) | `short_firetrail` on `pdp1`; sequence 3 loops `pdp1` active forever |
| `pdpanel2` | `pdp2` (+1.90) | `small_fireball_follow`, then `short_firetrail` at +2.01 s, on `pdp2` |
| `pdpanel3` | `pdp3` (−3.67) | `small_fireball_follow`, then `loop_short_firetrail` at +2 s, on `pdp3` |
| `pdpanel4` | `pdp4` (−1.32), `pcdp4` | 1st person: `gimme_bigflakes` + `large_firetrail` on `pcdp4`; else `gimmeflakes` + `short_firetrail` on `pdp4` |
| `pdpanel5` | `pdp5` (−1.70) | `yellow_sparks_follow`, `small_fireball_follow` +0.25 s, `short_firetrail` +1.5 s |
| `pdpanel6` | `pdp6` (+1.19), `pcdp6` | as `pdpanel4` on `pdp6`/`pcdp6` |
| `pdpanel7` | `pdp7` (z −2.02) | `small_fireball` + four `gimmeflakes` at ±2 x offsets |
| `pdpanel8` | `pdp8` (z +1.95) | `small_fireball` + two `gimmeflakes` at ±2 |

(x positions: the player airframe's `Object3d` translation under
`player_damage_on`; x < 0 is port, the convention the C2 finding measured.) So
**port-wing fire is `pdpanel3`/`4`/`5` at ≤0.15 / ≤0.30 / ≤0.50, starboard is
`pdpanel2`/`1`/`6` at ≤0.15 / ≤0.30 / ≤0.40** — which matches the owner frame
(port tip burning, smoke behind). `random_gun_impact`
(`yellow_sparks_follow` on `pdp1`/`pdp2` at 0.4/0.4, and always `pdp4`) is
what the `…_damage_effects`, `…_green`, `…_yellow` entries call; `…_red` only
cycles the gauge texture (`OBJECT_CYCLE_TEXTURE <part>damage RESET 3`; green =
`RESET 1`, yellow = `RESET 2`, the reset state is `RESET 0`).

**Direction of the threshold: inferred, not measured.** The list is stored
descending and the entries are named green → yellow → red, so the thresholds
read as *fraction of health remaining* crossing downward (0.99 = first hit,
0.15 = nearly dead). The entry itself stores no comparison. The second number
`25 25` after a part's name is unexplained.

## Emitter parameters (`PUFFER_STATE`)

Keywords: `NAME`, `ACTIVE_STATE` (`ACTIVE`/`INACTIVE`), `PRIORITY`, `AT_NODE`
(node + 0/3/6 words), `DISTANCE_INTERVAL` or `TIME_INTERVAL`, `LOCAL_VELOCITY`,
`WORLD_VELOCITY`, `MIN_`/`MAX_RANDOM_VELOCITY`, `WORLD_ACCELERATION`,
`FRICTION`, `SIZE_RANGE`, `NUMBER`, `LIFETIME_RANGE`, `GROWTH_FACTOR`,
`DEVIATION_DISTANCE`, `FADE_RANGE`, `NEAR_FADE`, `TEXTURES` (cycle) or
`TEXTURE_SEQUENCE` ((t name) frames), `COLORS` ((t r g b a) keys, rgb 0..255),
`START_TIME` ((clock s)). `INPUT_NODE` is the node the caller passes (`WITH_NODE`).
Textures: `smokepuffer` = `smoke101..103` (colour keys), `firepuffer` =
`fire_f01..f06` (frame times), fuel = `smoke101..103`, flak = `splashbase` /
`thickblksmoke01..03`.

Per effect (`pufftrails.zrd` unless noted). Columns: spacing, size, life, growth.

| effect (`id`) | emitter | spacing | size | life | growth | other |
| --- | --- | --- | --- | --- | --- | --- |
| `dense_firetrail` | `smokepuffer` | dist 1 | 0.4–0.8 | 3.5–4.5 | 2.45 | rand vel −0.1³ both min/max, fade 700–800, colours (0, 255 164 90, 1) (0.05, 5 5 5, 0.8) (0.2, 5 5 5, 0.8) (1, 0 0 0, 0) |
| | `firepuffer` | dist 0.25 | 0.1–0.4 | 0.5–0.7 | 2.25 | frames t = 0, .05, .15, .25, .3, .35 |
| `short_firetrail` | `shortpuffer1..4` cascade | dist 0.5 / 0.2 / 0.18 / 0.1 | 0.1–0.15 / 0.1–0.12 / 0.07–0.09 / 0.03 | 0.5–0.7 / 0.3–0.7 / 0.5–0.7 / 0.5–0.7 | 4.45 | stage 1 fire frames, 2–4 smoke; each stage switched off at +8 / +6 / +4 / +2 s then the next starts; then 50 % `black_smoke` (time 0.15, size 0.1–0.15, n 1) |
| `loop_short_firetrail` | `shortpuffer` | dist 0.5 | 0.1–0.15 | 0.5–0.7 | 4.45 | fire frames, looped by the node |
| `large_firetrail` | `lgpuffer` | dist 2, **priority 6** | 1.2–2.6 | 0.3–2.5 | 2.25 | fire frames, fade 900–1100 |
| `fuel_trail` | `fuelpuffer` | dist 0.1 | 0.05–0.1 | 0.5–1 | 2.45 | colours (0, 255³, 0.6) (1, 0³, 0); `LOOP_COUNT 10` |
| `sputter_firetrail` | `smokepuffer` + `firepuffer` | dist 2 / 0.1 | 0.8–1.5 / 0.2–0.6 | 1.5–4.5 / 0.3–0.5 | 1.45 / 0.25 | fire at `INPUT_NODE 0 0 0.2`; rand vel ±0.8; random switch-off |
| `engine_start_smoke` | `smokepuffer` | dist 1 | 0.8–1.5 | 1.5–4.5 | 1.45 | colours (0, 5 5 5, 1)…; off at +0.5 s |
| `small_fireball_follow` (host `small_injure_fireball`) | `lgpuffer` / `lgpuffer2` | dist 1.5 | 0.5–0.9 / 0.3–0.8 | 0.3–1.8 / 0.5–2 | 2 / 1.5 | priority 6; at `INPUT_NODE 0 0 −1` / `0 −0.25 −1`; colour (0, 255³, 0.6)(0.9, 255³, 0.8) / (150 100 100, 0.6→0.5); sound `snd_exp_hit5`; off at +1 s |
| `short_fire_follow` / `short_fireball_follow` | `lgpuffer` | dist 1 | 0.3–0.7 / 0.2–0.6 | 0.7–1 / 0.7–1.5 | 1.25 | priority 6; fade 800–900; off at +1.5 / +2 s |
| `sputter_black_smoke` (`partial_damage.zrd`, host `partial_damage`) | `black_smoke` | time 0.15 | 0.5–1.5 | 1–1.5 | 3 | n 5, local/world vel (0 5 0), rand vel (−10 2 −10)…(10 5 10), friction 0.9, near fade (70 30), frames fire_f05–f06 |
| `sputter_fire_smoke` | `black_smoke` | time 0.13 | 1–2 | 1–2 | 3 | n 3, world vel (0 10 0), friction 0.8, frames f03–f06 |
| `large_30sec_fire` / `huge_30sec_fire` (`flame_ball.zrd`, hosts `fire_here`/`fire_here2`) | `fire_n_smoke` | time 0.1 / 0.15 | 1–3.5 / 4–6 | 3.5–5.5 / 5.5–7.5 | 2.5 | fade 1500–1700 / 1000–1200; stopped after 30 s |
| `large_black_smokeball` (`flame_ball.zrd`, host `black_smoke_ball_01`) | `trailpuffer2` | time 1 | 4–7 | 4–5 | 4 | **n 5**, rand vel ±65, friction 8, fade 600–900, `thickblksmoke01..03`, off at +2.1 s |
| `flak` (`missile_puffers.zrd`) | `trailpuffer_dark` | dist 2 | 0.3–0.9 | 3.5–5 | 0.25 | texture `splashbase`, colours (0, 255 180 0, 0.8)(0.05, 50 50 50, 0.6), fade 400–600, off at +10 s |

The **flak puff of the owner frames** is `flak_effect` (`flak_control.zrd`,
host `flak_control`): `flak_flashes` (a `flak_light1` light, range 4–10, colour
(0.95, 0.76, 0.24), range animated +16/+40 over 0.3 s then −14/−34 over 0.1 s),
`call_flak_trails` (`flak_trails.zrd`: five `flak_trail1..5` objects thrown by
`OBJECT_MOTION` with `GRAVITY LOCAL −1..−3`, `RUN_TIME` 2–3 s, each with a
`partN_trail`) and `large_black_smokeball` at the same node; if the player is
within `PLAYER_RANGE 100`, a six-step `FBFX_COLOR_FROM_TO` screen flash.

The light that goes with the wing fire: **`fire_lt`** is a `LIGHT_STATE`
(`ACTIVE`, `SATURATED FALSE`) `AT_NODE prop1`, range (0.5, 2.25) colour
(0.88, 0.78, 0.36) or range (0.9, 3.25) colour (0.93, 0.78, 0.36), each
visible 0.1 s per flicker (weights 0.2/0.2 in `player_smoketrail`, 0.2/0.2 in
`player_firetrail`, 0.5/rest in `player_damage_trail`).

### `fire_lt` is not a scene node (the earlier unknown)

The compiled `player_smoketrail` record (`ZBD/C3/cam_anim.zbd`, record 121)
lists in its **nodes table** `wing_flare2`, `prop1` (both resolved to scene
nodes: id words `0x1e1d`, `0x1e1f`) and `fire_lt` with root-index word
`0xffffffff` (unresolved), and in its **lights table** one entry named
`fire_lt`. So `fire_lt` is a light the animation creates itself and places at
`prop1` through `AT_NODE`; it has no mesh and no airframe position of its own.

## Compiled form: where the floats sit (`ZBD/C3/cam_anim.zbd`, record 228)

The record `dense_firetrail` has `puffers: 3` (slot 0 empty, `smokepuffer`,
`firepuffer`; 44-byte entries) and one 2 496-byte sequence. Each puffer's
event is **596 bytes** (size word at event +4; the name field starts at +12);
checked bit-exact by `accept_parity2_dmg_fx_a_retail_cam_anim_…` against the
source for both puffers:

| event offset | field |
| --- | --- |
| +0x60 / +0x6c | `MIN_RANDOM_VELOCITY` / `MAX_RANDOM_VELOCITY` (3 × f32) |
| +0x88 | `DISTANCE_INTERVAL` |
| +0x8c, +0x90 | `SIZE_RANGE` |
| +0x94, +0x98 | `LIFETIME_RANGE` |
| +0xa4 | `DEVIATION_DISTANCE` |
| +0xb0, +0xb4 | `FADE_RANGE` |
| +0xb8 | `FRICTION` |
| +0x230 | `GROWTH_FACTOR` |
| +0xd0… (stride 36) | texture frames: 32-byte name; the `TEXTURE_SEQUENCE` time precedes its name (frame 1's time at +0xf0) |
| +0x1a4 (count 4), +0x1a8… | `COLORS` keys (t r g b a, 5 × f32 each; a zero first `t` is not shown by the byte scan) |

Unmapped in the event: the words at +0x2c (1 = smoke, 2 = fire: the puffer
table slot), +0x30 (`0x67efe`/`0x27efe`), +0x34 (−200), +0x38, +0x84,
+0x220, +0x228/+0x22c (1.0, 1.0). Event framing belongs to the F20 grammar.

## Propeller blur (`plane_props.zrd`, `spinprops` / `stopprops`)

One definition for every airframe (host `warhawk`; `spin_props_anim`
`spinprops`, `stop_props_anim` `stopprops` in the vehicle records).
`spinprops`: `prop1` ACTIVE, `nitroprop1` and `staticprop1` INACTIVE, then
`OBJECT_MOTION prop1 XYZ_ROTATION (0 0 −220 0 0 0)` looping forever from +0.1 s;
`prop1b` ACTIVE, rotating `(0 0 +60 …)` forever (opposite sense, 3.7× slower);
the same for `prop2/2b` and `prop3/3b`. `stopprops`: `staticprop1..3` ACTIVE
and faded in over 2 s (`OBJECT_OPACITY_FROM_TO` 0 → 1), `prop1..3`/`prop1b..3b`
faded out 1 → 0 over 1.5 s then INACTIVE; `slow_rotor1..3` spin the static
discs down in three `XYZ_ROTATION` steps (800/−400 over 2 s, −200/400 over
0.5 s, 100/−400 over 0.25 s). Positions (player airframe, `dontmove`): `prop1`
z 3.865, `prop1b` z 4.350, `nitroprop1` z 3.805, `staticprop1` at the origin.
`prop1` and `prop1b` both use `rotorblur.tif` (C-finding for #1667).

- **The ring detail is not in a record.** No ring, circle or concentric element
  exists in `spinprops`; the visible rings, if any, are texture content of
  `rotorblur.tif` or an interference of two discs turning at −220 and +60 (a
  hypothesis, not tested). The radial alpha profile of `rotorblur.tif` is not
  measured here. Resolving task: #1807, which loads the texture.
- The unit of `XYZ_ROTATION` (degrees per second is the plausible reading) is
  not stated beside the numbers.

## Unknown, recorded (rule 4)

1. **Units and curves.** `DISTANCE_INTERVAL` vs `TIME_INTERVAL` (world units vs
   seconds is the spelling, not a measurement), `GROWTH_FACTOR` (multiplier over
   life? per second?), `FADE_RANGE` (distances 400–1700 and `NEAR_FADE` (70 30)
   read as camera distances in unknown units), `FRICTION`, `DEVIATION_DISTANCE`,
   whether `TEXTURE_SEQUENCE`/`COLORS` times are seconds or lifetime fractions
   (frame times reach 0.35 against lifetimes 0.5–0.7; colour keys reach 1.0
   against lifetimes up to 4.5, which suggests a fraction for colours but is
   not proof), and how `NUMBER` combines with the interval.
2. **Threshold direction** (above) and the unexplained `25 25`.
3. **Who calls** `player_firetrail` and `player_damage_trail`, and the
   `RANDOM_WEIGHT` semantics (`IF (RANDOM_WEIGHT 0.4) … ELSEIF (… 0.4) … ELSE`
   reads as a weighted pick but is not verified).
4. **`PRIORITY`** (only 6 occurs on `lgpuffer`s) and `EXECUTION_PRIORITY`.
5. **Side of a trail.** `wing_flare2` is set INACTIVE by `player_smoketrail`
   and `player_fuelleak`: the starboard flare quad goes dark while the trail
   plays, and no definition touches `wing_flare1`. Why is not measured.
6. **nose/tail ↔ ±z.** `pdp7` is at z −2.02 and `pdp8` at +1.95 while `prop1`
   is at +3.87; which end `nose`/`tail` name is not concluded (the wing panels
   are consistent on x).
7. The `AT_NODE` six-word form (`pdp1 0 0 0 0 0 0`): offset + rotation is
   plausible, unmeasured.
8. The compiled-event words listed under "Unmapped".

Not claimed anywhere above: the engine's evaluation order, emission-rate
formula, blending mode, sprite orientation, or that any of this matches the
owner's frames. The owner screenshots were used to *choose* which records to
read (port wingtip fire, dark smoke, flak puff), never to set a number.

## Evidence

`cargo test --locked -p cs_formats -- accept_parity2_dmg_fx_a_ --include-ignored`
runs 12 tests (5 synthetic, 7 retail). The retail ones decode all 221 `.zrd`
members of `ZBD/zrdr.zbd` (162 animation members, 616 definitions, exact
counts), assert the exact numbers in the tables, the vehicle ladders, the
`fire_lt`/`prop1`/`pdpN` chain and the propeller rotations, and cross-check the
floats against the compiled `cam_anim.zbd` record at the offsets above.

Follow-ups this unblocks: #1737 (wing fire and smoke trails), #1806 (flak and
explosion puffs), #1807 (propeller blur disc), #1856 (crash fire and sparks).
