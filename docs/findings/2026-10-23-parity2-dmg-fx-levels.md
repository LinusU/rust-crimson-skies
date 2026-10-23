# PARITY2-DMG-FX-LEVELS: which damage effect plays at which level, how the puffer runtime applies its numbers, what starts the prop discs

Date: 2026-10-23. Task: #1979 (prerequisite of #1737). Capabilities used: `retail` (read-only `$CS_GAME_DIR`) and
`$CS_ENGINE_IMAGE` (read-only, never committed; disassembled with `llvm-objdump` into `private/`, which Git
ignores). The original executable was not run: every claim below is `static` (image) or `data` (the retail
`.zrd` members read through the production readers), and nothing is `verified_original`. Addresses are virtual
addresses in the image (file offset = `va - 0x400000`). Implementer: `sonnet-1` (Sonnet 5.5); no independent
review yet.

Builds on `2026-10-21-parity2-dmg-fx-a-effect-definitions.md` (the definitions), `...-b-burst-puffs.md`,
`...-c-propeller-blur-profile.md`, `2026-10-23-parity-m01-player-damage-trail.md` and
`2026-10-23-parity2-dmg-fx-d-damage-frame-beside-the-original.md`.

## 0. What #1737 can implement now

1. **The level rule is measured** (section 1): every `injure_anims` entry is an independent switch on a
   *health-remaining fraction*. It runs while `fraction <= threshold` and stops when the fraction rises above it.
   There is no "pick one level": at 8 % health the 0.85 and the 0.10 vehicle entries and every part entry at or
   above the part's own fraction all run at once. The vehicle fraction is the sum over the four parts, so the M01
   player (four parts of 25) enters `player_fuelleak` after 15 points of damage and `player_smoketrail` after 90.
2. **The wing fire is the part ladder, not `player_smoketrail`.** The port-wing fires `pdpanel5/4/3` start at
   leftwing health fractions 0.50 / 0.30 / 0.15 and the starboard ones `pdpanel6/1/2` at 0.40 / 0.30 / 0.15
   (section 2 table). `dense_firetrail` on `prop1` (`player_smoketrail`, the plume the damage-trail runtime draws
   today) needs the *vehicle* total at 0.10 or below. Gating the plume on "any wing region damaged"
   (`damage_trail.rs`) is therefore wrong in both directions.
3. **None of these animations moves the airframe, a wing, a panel or a prop node.** They switch node visibility
   (`pdp1..8`, `pcdp4/6` on; `wing_flare2`, `winglight2`, `fire_lt` off), cycle the gauge texture, throw debris
   (`flake1..7`, `bit1..3`, `chunk`) and spawn puffers (section 2). Nothing in them is a skeletal or damage-pose
   motion.
4. **Puffer numbers** (section 3): `GROWTH_FACTOR` G is a size multiplier running linearly from 1 to G over the
   puff's life; `FADE_RANGE a b` is a camera-depth ramp (opaque to `a`, linear to nothing at `b`, culled from `b`);
   `COLORS` keys and `TEXTURE_SEQUENCE` frame times are **fractions of the puff's life** (the damage-trail runtime
   reads the frame times as seconds, which is wrong); with no `COLORS` the opacity is `(1 - life fraction)`;
   `FRICTION` is `exp(-friction * dt)` (a 1/51-step table) on the velocity relative to the wind.
5. **The prop disc has no speed rule to implement.** The spin / stop animations are chosen by one engine-on bit,
   edge-triggered, not by RPM (section 4). While spinning the data makes *both* `prop1` and `prop1b` active and
   sets no opacity; `prop1b` is the only data-backed difference from what is drawn today. There is no ring rule in
   code or data.

## 1. The selector: `0x4b3800` (vehicle) and `0x4b3d70` (part)

The `injure_anims` keyword is parsed twice from the vehicle record (string `0x627dc4` at the part parse, push
`0x4796a5`; string `0x627eb4` at the vehicle parse, push `0x479f2d`). An entry is **0x1c bytes**: `float`
threshold at +0, the animation-name string at +4, the resolved node at +0x14, the animation index at +0x18.
The vehicle list is at the vehicle record +0x1a8 (end +0x1ac); a part's list at the part object +0x3c (end
+0x40) with its running-animation handles at +0x4c; the vehicle's handles at vehicle +0x890.

| selector | fraction | rule (claim `parity2-dmg-fx-levels.selector-rule`) |
| --- | --- | --- |
| `0x4b3800` vehicle | `[+0x2d0] / [+0x2cc]` (`0x4b3806`, `0x4b380e`) | per entry: if a handle exists and `x <= thr` (`fcomp; test ah,0x41; jne skip`, `0x4b386b`) leave it; if `x > thr` stop it (`0x4ed480`, `0x4b3877`); if no handle and `x <= thr` (`...; je skip`, `0x4b38a3`) start it (`0x4edda0`, `0x4b38e9`) |
| `0x4b3d70` part | `[+0x30] / [+0x2c]` (`0x4b3d76`, `0x4b3d7a`) | the same two branches (`0x4b3dc2`, `0x4b3dee`; stop `0x4b3dca`, start `0x4b3e03`) |

* **Direction (was unknown): health remaining.** The animation plays while `fraction <= threshold`; a fraction
  exactly equal to the threshold plays (the equal case sets `C3`, which the `test ah,0x41` catches).
* **Independent switches.** The loop visits every entry on every call and gives each its own handle, so any
  number of entries run together; an entry whose threshold is above the current fraction is already running and
  is left alone. The 0.99 entries of a part start at the first point of damage.
* **Which fraction.** The part-health setter `0x47bd90` writes a part's four numbers (+0x24/+0x28 and +0x2c/+0x30
  of the 0x58-byte part) and re-sums over all parts (`0x47bedf..0x47bf4a`): `+0x2c` into the vehicle's `+0x2cc`
  and `+0x30` into `+0x2d0` (and `+0x24`/`+0x28` into `+0x2c4`/`+0x2c8`). That `+0x2cc` is the maximum and
  `+0x2d0` the current is inferred, not read from a name: the setter initialises both of each pair to the same
  number (`25 25` in the record), and a flagged drain at `0x48adeb` subtracts `dt * [+0x2cc] * 0.5` from
  `+0x2d0` and kills the vehicle at or below 0. The role of the two numbers `25 25` is still not traced.
* **When.** Event-driven, not per frame: the vehicle selector is called from 11 sites and the part selector from
  7 (listed in the test); the setter's tail calls the part selector (`0x47bf50`) and then the vehicle one
  (`0x47bf57`).

Vehicle-level entries (`vehicle.zrd`, all eleven `player_*` airframes carry the same two):

| threshold | animation | what it does (data, `2026-10-21-...-a`) |
| ---: | --- | --- |
| 0.85 | `player_fuelleak` | weighted pick of `3040slug_gunhit` + `fuel_trail` at `pdp1` / `pdp2` / `pdp3` (the `pdp3` branch also sets `wing_flare2`, `winglight2` inactive) |
| 0.10 | `player_smoketrail` | `dense_firetrail` on `prop1`, `wing_flare2` inactive, `fire_lt` flicker 50 loops |

The 11 player airframes differ only in their part ladders (thresholds and which `pdpanel` sits on which step,
e.g. `pfirebrand` leftwing 0.30 / 0.19 / 0.15); the M01 player `pdevastator` is the table below. Non-player
airframes (`devastator`, `rdevastator`, ...) carry a vehicle-level ladder of `random_remote_damage` at 0.95 / 0.80
/ 0.65 / 0.50 / 0.45 / 0.25 plus one trail at 0.40 (`pfsmoketrail`, `firebrand_trail`, `kestrel_trail`,
`warhawk_trail`) and no part ladders; those animations were not read here.

## 2. The M01 player's ladders and what they move

Part fractions of `pdevastator` (four parts, `25 25` each; thresholds descending as stored, each an independent
switch):

| part | 0.99 | 0.72 | 0.50 | 0.46 | 0.40 | 0.30 | 0.20 | 0.15 |
| --- | --- | --- | --- | --- | --- | --- | --- | --- |
| `leftwing` | `_effects` | `_green` | **`pdpanel5`** | `_yellow` | | **`pdpanel4`** | `_red` | **`pdpanel3`** |
| `rightwing` | `_effects` | `_green` | | `_yellow` | **`pdpanel6`** | **`pdpanel1`** | `_red` | **`pdpanel2`** |
| `nose` | `_effects` | `_green` | | `_yellow` | | | `_red` | **`pdpanel7`** |
| `tail` | `_effects` | `_green` | | `_yellow` | | | `_red` | **`pdpanel8`** |

`_effects` / `_green` / `_yellow` / `_red` are the gauge-texture cycles (`OBJECT_CYCLE_TEXTURE`) plus
`random_gun_impact` sparks; the `pdpanelN` animations are the fires of `2026-10-21-...-a` (node, puffers).

* Closure of every animation these entries call (38 animations, test
  `..._retail_injure_animations_move_only_debris_nodes`): event vocabulary `ACTIVATION, CALL_SEQUENCE, ELSE,
  ELSEIF, ENDIF, IF, LOOP, OBJECT_ACTIVE_STATE, OBJECT_CYCLE_TEXTURE, OBJECT_MOTION, SOUND, STOP_SEQUENCE,
  PUFFER_STATE, LIGHT_STATE`. `OBJECT_MOTION` moves only `flake1..7` (`gimmeflakes`, `gimme_bigflakes`) and
  `bit1`, `bit2`, `bit3`, `chunk` (`3040slug_gunhit`). `OBJECT_ACTIVE_STATE` sets `pdp1..8`, `pcdp4`, `pcdp6`,
  `flame_ball_02` active and `fire_lt`, `gunhit_lt`, `wing_flare2`, `winglight2`, `yellow_spark_02`, `flame_ball_02`
  inactive. **`wing_flare1` / `winglight1` are never touched** and no airframe, wing or prop node is moved.
* So the wing damage the original shows is puffers and lights at fixed nodes plus node visibility; the wing
  itself does not deform or move.
* `PLAYER_1ST_PERSON` branches in `pdpanel4` / `pdpanel6` (`large_firetrail` on `pcdp4`/`pcdp6` in the cockpit,
  `short_firetrail` on `pdp4`/`pdp6` otherwise) and `RANDOM_WEIGHT` picks are conditions the selector does not
  evaluate; their semantics are not traced here.

## 3. The puffer runtime (what `GROWTH_FACTOR`, `FADE_RANGE`, `FAR_FADE` do)

Pieces: the event applier `0x4e7e60` (compiled event to runtime emitter), the emitter update `0x54f8b0`, the
puff update `0x54ee10`, the puff draw `0x54e6e0`, the emitter setters `0x550370..0x550690`.

**Event to emitter** (claim `parity2-dmg-fx-levels.applier-map`): the applier tests the event's flag word (+0x30)
and calls one setter per field, in a fixed order (`0x4e7f69..0x4e81dd`, all re-read by the test):

| event field (parse offset) | keyword | emitter |
| --- | --- | --- |
| +0x84 / +0x88 | `DISTANCE_INTERVAL` / `TIME_INTERVAL` | distance-mode byte `+0x3c`, interval `+0x40`, `1/interval` `+0x44` |
| +0x8c, +0x90 | `SIZE_RANGE` | `+0x4c`, `+0x50` |
| +0x94, +0x98 | `LIFETIME_RANGE` | `+0x54`, `+0x58` |
| +0x9c, +0xa0 | `START_AGE_RANGE` | `+0x5c`, `+0x60` |
| +0xa4 | `DEVIATION_DISTANCE` | `+0x64` |
| +0xa8, +0xac | `NEAR_FADE` | `+0xd0`, `+0xd4`, `1/(b-a)` `+0xd8` |
| +0xb0, +0xb4 | **`FADE_RANGE`, also spelt `FAR_FADE`** (two lookups of one field, flag 0x1000) | `+0xdc`, `+0xe0`, `1/(b-a)` `+0xe4` |
| +0xb8 | `FRICTION` | `+0x68` |
| +0xbc | `WIND_FACTOR` | `+0x6c` |
| +0xc0 | `PRIORITY` | `+0x70` |
| +0xc4 | `NUMBER` | `+0x04` |
| +0x220 count, +0x224.. pairs | `SCALE_SEQUENCE` **and `GROWTH_FACTOR`** (flag 0x2000) | scale sequence |

**`GROWTH_FACTOR` G** (claim `parity2-dmg-fx-levels.growth`): the parser stores G at event +0x230 and, in the
same branch, writes the scale sequence `count 2; (key 0.0, scale 1.0); (key 1.0, scale G)` into +0x220..+0x230
(`0x4fafbb..0x4fafd5`; row `puffer_growth_start_scale`). The draw looks the sequence up with the puff's
**normalised age** `t = age * (1/lifetime)` (`0x54e7c7`) and multiplies the size by the linear interpolation, so
`size(t) = SIZE * (1 + (G - 1) * t)` with `SIZE` drawn once at spawn from `SIZE_RANGE`. An effect without
`GROWTH_FACTOR` and without `SCALE_SEQUENCE` has an empty sequence and scale 1. (`dense_firetrail`, G 2.45: a
0.4-0.8 puff ends 0.98-1.96 across; `short_firetrail` 4.45.)

**`FADE_RANGE a b`** (claim `parity2-dmg-fx-levels.fade-range`): with `z` the puff's view-space depth (the z component of the camera
transform of its position, `0x54e708..0x54e720`),
`d = z * [0x9fd5c0]` and `s = d * [0x637a94]` (the global fade factor, default 1.0, script
`PufferSetGlobalFadeFactor`):

* `s >= b`: not drawn (`0x54e73c`); `a < s < b`: opacity `x (b - s) / (b - a)` (`0x54e75f`); `s <= a`: x 1.
* The defaults are `a = b = f32::MAX` (row `puffer_far_fade_default_bits`), so a puffer without the keyword
  never fades by distance. `dense_firetrail` carries `700 800`, `large_firetrail` `900 1100`,
  `short_fire_follow` (`lgpuffer`) `800 900`.
* **Unit: not settled.** `[0x9fd5c0]` is the 7th argument of the camera set-up `0x53c110` (also the numerator of
  the perspective divide at `0x53d8dc`); its caller `0x540baf` passes the camera state's `+0xb0`, which the fog
  finding (`2026-10-23-m01-fog-update-static.md`) identifies as the camera clip pair's first member. If that value
  is 1.0 in M01 the numbers are world units; its M01 value was not read. Until it is, `a`/`b` are "depth times
  that factor".

**`NEAR_FADE a b`**: culled at `d <= a` (`0x54e795`), opaque at `d >= b` (`0x54e7a7`); between, the ramp is
`(d - [+0x3c]) * [+0x44]` (`0x54e7b5`), which subtracts the *far start*, not `a`, so it is not positive (the
far start is `>= d` in every case where it is set, and `f32::MAX` otherwise) and the puff is culled. In effect
`NEAR_FADE` is a hard cut at `a` (`sputter_black_smoke` carries `70 30`: culled at 70 or nearer). Only the
sputter smoke uses it; none of the M01 damage trails do.

**Other measured rules the existing runtimes apply as designed** (all `static`):

* *Emission* (`0x54f8b0`): `DISTANCE_INTERVAL` accumulates the anchor's travel since the last frame in world
  units, ignoring a move of 200 or more (row `puffer_jump_guard_distance`); `TIME_INTERVAL` accumulates the frame
  step. The emitter spawns `floor(accumulator / interval)` times `NUMBER` puffs; puff i of an emission sits
  `i * interval / accumulator` along the path and starts `(1 - that) * dt` old, plus `START_AGE_RANGE`; a puff whose
  start age already reaches its lifetime is dropped. Position gets `(rand - 0.5) * DEVIATION_DISTANCE` per axis.
  A `TEXTURES` list (no sequence) is indexed by `rand() % count` at spawn.
* *Age* (`0x54efc1`): `age += [0x637a90] * dt` (default 1.0, script `PufferSetGlobalAgeFactor`); a puff dies at
  `age >= lifetime`.
* *Friction*: `v' = w + (v - w) * g(dt * FRICTION)` with `w = wind * WIND_FACTOR` and
  `g(x) = exp(-x)` read from a 256-entry table at steps of 1/51 (`0x53e2e0`: 1 below 0, 0 above 5; rows
  `puffer_friction_*`). With no wind this is the `v * exp(-FRICTION * t)` the runtimes use, to the table's step.
* *Colour and opacity* (`0x54e9d9..0x54ee80`): `COLORS` keys are normalised-age fractions (the parser refuses
  keys that are not strictly increasing, string "age values not strictly increasing"), interpolated linearly in
  rgb (0..255 as stored) and alpha; the opacity is the interpolated alpha times the distance fade. **With no
  `COLORS` the puff is white and its opacity is `(1 - t)` times the fade.**
* *Frames*: `TEXTURE_SEQUENCE` times are also compared with the **normalised age** (`0x54ecb0`: the frame cursor
  advances while `t >= next key`), so the `0, 0.05, 0.15, ...` of `firepuffer` are fractions of the 0.5-0.7 s
  life, not seconds. `damage_trail.rs` reads them as seconds.
* *Alpha*: the final opacity times 255 (row `puffer_alpha_scale`), plus 0.5, clamped.
* *`PRIORITY`*: the draw multiplies the reciprocal depth by `1 + [0xa06fb0] * PRIORITY` (`0x54e8e9`; the factor
  is 0.02 or 0.01 depending on a boot branch, rows `puffer_priority_depth_bias_*`). Whether that scales the
  sprite, biases its depth or both is not separated; for the `lgpuffer`s at priority 6 it is 1.12 or 1.06.

## 4. The propeller discs

* **What starts them.** The vehicle record fields `spin_props_anim` / `stop_props_anim` (strings `0x6280dc`,
  `0x6280ec`) become indices at `+0x18c` / `+0x190` (`0x47b13c`, `0x47b163`). `0x4b1630` plays the spin animation
  (reads `+0x18c`, kills a running stop animation), `0x4b15c0` plays the stop animation (reads `+0x190`, kills the
  spin one). The flag-word setter `0x4b1690` toggles bit `0x2` of the vehicle word `[+0x2dc]` and on the edge
  calls one of them (`0x4b1718..0x4b1739`); spawn calls the spin function (`0x47b9ac`), death the stop function
  (`0x4b8440`). **Neither the speed, the throttle nor an RPM is read anywhere on that path.** Which of the setter's
  ten other call sites sets bit `0x2` (engine on / off) was not decoded (bit `0x1` is the "health below
  `record+0xbc`" flag at `0x4806cc`).
* **What the animation does** (data, `...-a`): `spinprops` makes `prop1`, `prop1b`, `prop2`, `prop2b`, `prop3`,
  `prop3b` active and moves `prop1` / `prop1b`, makes `nitroprop1` and `staticprop1` inactive, and contains no
  opacity event; `stopprops` is where the timed opacity fades live. So while spinning a disc is drawn at its
  node's own opacity and its texture alpha (the node default and the blend equation of the textured disc were
  not read).
* **Ghosts and rings.** There is no ring or ghost rule in code or data. The offset copies of the owner's frame are
  consistent with the second disc `prop1b` (0.485 m behind `prop1`, the other direction of spin, same
  `rotorblur.tif`) being drawn; the rings with the texture's own red annulus at 0.6 of the radius
  (`...-c-propeller-blur-profile.md`). Two stacked discs of at most 22 % coverage are at most about 39 % opaque;
  whether that is the original's "darker" look is for the owner's eyes (`human_review`), not claimed.

## 5. Claims and evidence

| claim id | what | evidence | class |
| --- | --- | --- | --- |
| `parity2-dmg-fx-levels.selector-rule` | entry runs while `fraction <= thr`, independent switches, event-driven | `0x4b3800`, `0x4b3d70`, call sites; test `..._image_injure_anims_selector` | `static` |
| `parity2-dmg-fx-levels.fraction` | vehicle fraction = sum of part `+0x30` over sum of `+0x2c`; part fraction `+0x30/+0x2c` | `0x47bf0e..0x47bf4a`, `0x4b3d76`; same test | `static` (cur/max naming inferred) |
| `parity2-dmg-fx-levels.ladders` | the per-airframe ladders and what the closure moves | `vehicle.zrd`, `player.zrd`, `gunhit.zrd`, ...; tests `..._retail_vehicle_ladders`, `..._retail_injure_animations_move_only_debris_nodes` | `data` |
| `parity2-dmg-fx-levels.applier-map` | event field to emitter field | `0x4e7e60` call order, setters `0x550440..0x5505d0`; test `..._image_puffer_runtime` | `static` |
| `parity2-dmg-fx-levels.growth` | `GROWTH_FACTOR` = 2-key scale 1 to G over normalised age | `0x4fadb7..0x4fafd5`, `0x54e7c7`; same test | `static` |
| `parity2-dmg-fx-levels.fade-range` | `FADE_RANGE` / `FAR_FADE` ramp and culling | `0x55055c..0x550576`, `0x54e732..0x54e75f`; same test | `static` |
| `parity2-dmg-fx-levels.keys-are-life-fractions` | `COLORS` and `TEXTURE_SEQUENCE` keys are normalised ages | `0x54e7c7`, `0x54e93d`, `0x54ecb0` | `static` (read, not in a byte test) |
| `parity2-dmg-fx-levels.constants` | the eleven rows of `[[puffer_runtime]]` | test `..._image_rows_match_the_image` | `static` |
| `parity2-dmg-fx-levels.prop-trigger` | prop spin / stop by one engine bit, no RPM | `0x4b1690`, `0x4b1630`, `0x4b15c0`; test `..._image_prop_discs_follow_one_engine_flag` | `static` |
| `parity2-dmg-fx-levels.prop-data` | `spinprops` has no opacity event, `prop1b` active | `plane_props.zrd`; test `..._retail_spinning_props_carry_no_opacity_event` | `data` |

## 6. Unknown, recorded (rule 4)

1. **The unit of `FADE_RANGE` / `NEAR_FADE`**: `[0x9fd5c0]` for M01's camera (section 3).
2. **`RANDOM_WEIGHT`, `PLAYER_1ST_PERSON`, `ACTIVATION`, `LOOP_COUNT -1`** evaluation inside the animations.
3. **Which call sets bit `0x2` of the vehicle flag word** (engine on / off) besides spawn and death.
4. **The `25 25` pair** of a part and which of `+0x24/+0x28` versus `+0x2c/+0x30` the hit path decrements
   (the selector reads `+0x2c/+0x30` only).
5. **The blend equation and node-default opacity of the prop discs**, and the colour-space of the puff colours.
6. **`PRIORITY`'s exact effect** (size, depth or both) and `[0xa06fb0]`'s branch.
7. **Non-player vehicle ladders** (`random_remote_damage`, `pfsmoketrail`, `firebrand_trail`, ...): not read.
8. **Which of the owner frame's effects is which entry**: the port plume at +70 s is consistent with the port
   `pdpanel5/4/3` fires (a leftwing fraction of 0.5 or below) and with `dense_firetrail` (vehicle total 0.10 or
   below); this finding does not choose, and nothing was tuned to the frame.

## Code, data and tests

* `data/owner-static/engine-facts.toml`: eleven `[[puffer_runtime]]` rows (claim `OWNER-STATIC-2026-10-23`,
  `static`), each with its address.
* `crates/cs_formats/tests/zbd/accept_parity2_dmg_fx_levels.rs` (registered in `tests/zbd/mod.rs`): one test that
  runs everywhere (the rows are exactly this table), three retail data tests (`#[ignore = "requires CS_GAME_DIR"]`)
  and four image tests (`#[ignore = "requires CS_ENGINE_IMAGE"]`). Prefix `accept_parity2_dmg_fx_levels_`:
  `cargo test --locked -p cs_formats -- accept_parity2_dmg_fx_levels_ --include-ignored`.
