# FLAK-C1-TRACE (#1883): `wep_27`'s descriptor flag, the range end and the burst presentation

Date: 2026-10-22. Task #1883, a follow-up of `2026-10-21-flak-a-turret-update-law.md`
(table "Unknowns"). Capabilities used: a static read of `$CS_ENGINE_IMAGE` and a
read-only look at the records of `ZBD/zrdr.zbd` / `ZBD/C1/cam_anim.zbd` in
`$CS_GAME_DIR`. No original executable ran; every claim is provenance `static`,
none is `verified_original`. Nothing of the image is committed but cited bytes.

The cited instructions are the rows `[[flak_trace_fact]]` of
`data/owner-static/engine-facts.toml` (claim `OWNER-STATIC-2026-10-22`): each row
names the instruction's VA, its machine-code bytes and what it decides. The
acceptance tests `accept_flak_c1_*` re-read every row against the image (ignored
without `CS_ENGINE_IMAGE`) and the `wep_27` record against the installation
(ignored without `CS_GAME_DIR`). Names in `code` below are the rows' `name`.

## Answers

| Question (FLAK-A) | Answer |
| --- | --- |
| (a) Is `wep_27`'s descriptor flag `0x40` set? | **No.** `0x40` is the `CANNON` key; the `wep_27` record has none. |
| Does a clear flag stop a flak round from carrying its target? | **No.** The target reaches a `LOCK_ON` rocket through the lock globals, not through the flag. A turret flak round bursts at 20 m from its target. |
| (c) What does a rocket do at range end without `DETONATE_AT_RANGE`? | It depends on `LOCK_ON` and on weapon flag `0x80`, which is the `EXPIRES` key. `wep_27` (`LOCK_ON`, no `EXPIRES`) **bursts at `RANGE`**. |
| (b) Which `flak_effect` parts run at the burst? | The burst starts the default `IMPACT` row's animation at the burst position; its three parts are the animation's own events. The close-range flicker is the `IF PLAYER_RANGE` gate, law below. **One guard in the starting code is unresolved** (see "Unresolved"). |

## (a) The descriptor flag

`weapon+0x210` is a pointer to a 0x38-byte descriptor. The weapon constructor
zeroes it (`descr.ctor_clear`) and then calls the descriptor callback it was
given with `(record, weapon)` (`descr.ctor_callback`). The only caller of the
weapon-section parser `0x5ad630` (`descr.parser_call`, at `0x4647ef`) passes the
callback `0x4ba6f0` (`descr.parser_callback_arg`). That callback allocates the
descriptor with `calloc(1, 0x38)` (`descr.alloc`; the import at `0xa201b4` is
MSVCRT `calloc`, so every flag starts clear), stores it in `weapon+0x210`
(`descr.store`) and sets flags from keys of the weapon record.

Flag `0x40` is set by exactly one instruction in `0x4ba6f0..0x4bab50`:
`descr.cannon_flag`, guarded by the key at `0x62b320`, the string `CANNON`
(`descr.cannon_key`, `descr.cannon_string`). **Descriptor flag `0x40` = the
weapon record has a `CANNON` key.** The turret code reads it through
`weapon+0x210` (`descr.read_in_fire`, plus the rows of the FLAK-A finding).

The `wep_27` record (`weapons.zrd`, section `BALLISTICS`; read from
`ZBD/zrdr.zbd`) has these keys: `DESC` (`MSG_WEAP_AAFLAK_ROCKET`), `NAME FLAK`,
`ACCELERATION 150`, `AMMO_LIMIT 9999`, `CRATER 0`, `ARMOR_DAMAGE 10`,
`HEALTH_DAMAGE 10`, `DETONATION_DISTANCE 20`, `FIRE_RATE 3`, `GRAVITY 0`,
`IMPACT_PROXIMITY 22`, `LOCK_ON 3`, `LOCK_ON_LEAD (5 10)`, `RANGE 900`,
`TURN_RATE 0.001`, `VELOCITY 850`, `ROCKET`, `CLUSTER_SIZE 1`, `PRIORITY 5`, the
`FIRE`, `FLYOUT` and `IMPACT` sections. No `CANNON`, `EXPIRES`,
`DETONATE_AT_RANGE`, `DETONATION_TIME` or `TANGLER`. The turret gun `wep_29` is
the contrast: it has `CANNON`.

So for a turret firing `wep_27` (consequences for the FLAK-A law): no
intercept solver and no intercept requirement, no lead direction (the shot
direction is target minus muzzle, normalised, plus the `INACCURACY` cone), no
second cached ray, no lock-on record (`fire.lock_target_gate`), the engaged flag
is the 15-degree alignment alone.

### The target still reaches the rocket

Before every shot `turret.lock_globals` calls `0x5ac0a0(&entry+0x1ec,
&entry+0x1f0)` (`lock.globals_setter` stores them in `0xa1d7cc/0xa1d7d0`), flag
or not. While a target is held the update stores two values of the target
wrapper into the entry: `entry+0x1f4` = vtable slot 0 and `entry+0x1f8` = slot 1
(`turret.target_node_store`, `turret.target_slot1_store`). Slot 0 returns, per
wrapper, `obj+0x3a0` (vehicle), `obj+0x70` (turret, the mount node), `obj+0xc`
(struct) and `0x4416d0(obj)+0xc` (projectile) (`wrapper.*_slot0`); slot 1 is the
float at `obj+0x930` for a vehicle and 0.0 for the others.

The weapon fire `0x5aef40` creates the rocket entity with `rocket+0x70` cleared
(`rocket.target_clear`). For a weapon with flag `0x8000` (`LOCK_ON`) and a
non-null `0xa1d7d0` (`rocket.lock_copy_gate`; descriptor flag `0x40` is not
consulted) it copies the 12 bytes at `[0xa1d7d0]` (`entry+0x1f0..0x1fb`) into
`rocket+0x6c..0x77` if `[[0xa1d7cc]] > 0` (`rocket.lock_copy`). `entry+0x1ec` is
set to 1 by the constructor (`turret.lock_default`) and nothing in
`0x4a9000..0x4ac800` writes it again. **So `rocket+0x70` is the held target's
slot-0 pointer, and the proximity burst of FLAK-A (`rocket.proximity_burst`: the
rocket's position against `rocket+0x70` through `0x538880`, squared distance
`<= DETONATION_DISTANCE²`) applies to a turret's flak round: 20 m from the
target.** The pointee of slot 0 is read live each frame by `0x538880` as a
vector; what that vector is for each wrapper (position at the pointer's start)
was not traced further.

## (c) The range end

At `travelled >= RANGE` (`rocket.range_test`, FLAK-A) the code at `0x5b01f9`
decides, with weapon flags `L` = `0x8000` (`LOCK_ON`), `E` = `0x80` and `D` =
`0x2000000` (`DETONATE_AT_RANGE`):

| `LOCK_ON` | `EXPIRES` | `DETONATE_AT_RANGE` | At range end |
| --- | --- | --- | --- |
| no | any | yes | burst (`0x5b02ba`) |
| no | any | no | **silent end**: `rocket+0x20 = 0`, no impact call (`rocket.silent_end`) |
| yes | no | any | **burst** |
| yes | yes | yes | burst |
| yes | yes | no | silent end |

(`rocket.range_end_lock_expires`, `rocket.range_end_detonate_test`,
`rocket.burst_call`.) Flag `0x80` is the boolean key `EXPIRES`
(`weapon.expires_key`, `weapon.expires_flag`), which FLAK-A listed as unnamed.
`rocket.alive_set` sets `rocket+0x20 = 1` on every pass; `rocket.silent_end` and
the burst path both clear it. What the entity pool does with `rocket+0x20`
was not traced. **`wep_27` therefore bursts at 900 m when nothing triggered it
earlier**, held target or not.

## (b) The burst presentation

Every burst reason ends in `rocket.burst_call`: `0x5ac3a0(weapon, rocket)`. It
stores the burst position in the impact globals, builds a hit record on its
stack, zeroes the record's surface slot (`burst.record_zero`) and calls the impact
code `0x5ac7a0(weapon, record, rocket, 0)` (`burst.impact_call`); afterwards,
`IMPACT_PROXIMITY > 0` runs the sphere query of FLAK-A.

The `IMPACT` row index is `[[record+0x20]+0x20]` (`burst.row_index`), which is 0
for a rocket burst. The surface table (`surface.table`) numbers `default` 0,
`water` 1, `seafloor` 2, `quicksand` 3, `lava` 4, `fire` 5. **A proximity or
range burst of `wep_27` uses row 0, `default`: `ANIMATION flak_effect`, `SOUND
snd_missile_flak`.** The rows `water`, `quicksand`, `player` and `buildings` of
the record belong to impacts that pass a surface (other callers of `0x5ac7a0`);
the `player` row's `ANIMATION f18sparks2` and `EFFECT rcochet1` are not reached by
an air burst.

What `0x5ac7a0` does for this weapon (flags: `CRATER` present → weapon flag
`0x10`; no `TANGLER`, so `weapon+0x20c` is null and the hook flags are 0:
`burst.callback_hook`, `burst.tangler_key`, `burst.hook_set`, `burst.hook_setter`):

* `burst.crater_gate` / `burst.crater_call` / `burst.crater_node_gate`: with
  flag `0x10` the crater step `0x5ac690` runs and returns 0 unless the node
  `[record+0x24]` has mask `0x10000` at `+0x24`. For a burst `record+0x24` is the
  rocket's own node (`0x5ac3d2`). A non-zero result (a crater was placed) skips
  the ANIMATION and EFFECT starts (`burst.skip_anim_if_crater`). Whether the
  rocket node ever has mask `0x10000` was not traced; the assumption of this note
  is that it has not.
* `burst.sound_pick`: `0x5ad100(weapon, record, row, 1.0)` picks a sound by
  `rand` and plays it through `0x593b80` (`burst.sound_play`); `burst.row_sound`
  plays the row's own sound handle (`row+0x24`) at the burst position through
  `0x525d90`. The sound names and the choice law belong to FLAK-C4.
* `burst.anim_row` / `burst.anim_start`: the ANIMATION handle of the row is read
  at `weapon+0x15c + row*100 + 4` and started by `0x4edc10(handle, 0, x, y, z, 0
  ×6)` at the burst position, with no node (`burst.anim_wrapper` forwards to the
  animation start `0x4ed730`). See "Unresolved" for the guard in front of it.

### The `flak_effect` parts and the close-range flicker

`flak_effect` is the animation of `flak_control.zrd`; its three parts
(`flak_flashes`, `call_flak_trails`, `large_black_smokeball`) and the six-step
`FBFX_COLOR_FROM_TO` flash are described in
`2026-10-21-parity2-dmg-fx-a-effect-definitions.md`. This trace adds the law of
the gate in front of the flash:

* The text parser of the animation `IF` event reads `PLAYER_RANGE` at `0x516a93`
  (`if.player_range_key`, FLAK-A's `0x516a94`): it sets bit 2 of the event's
  flags (`if.flag`) and stores the value **squared** in `event+0x14`
  (`if.store_squared`). The compiled animation carries the squared value: the
  retail `ZBD/C1/cam_anim.zbd` `flak_effect` record holds the single float
  `10000.0` (and no `100.0`) in its bytes (asserted by the ignored retail test).
* The runtime condition evaluator is `0x4ec080` (`if.eval_threshold`). For bit 2
  (`if.eval_flag`) it needs the player-position word `0x9fd16c`, computes the
  squared distance from the animation's node `[anim+0x6c]` (its world position
  through `0x4cf200`) to the player position `0x9fd184..0x9fd18c` (`if.eval_distance`,
  `if.dist_sq`) and passes when it is **`<= event+0x14`** (`if.eval_compare`).
  So the flicker runs for a burst whose node is within **100 m** of the player.
* The player position is set by `0x4e7020` (`if.player_pos_setter`) from the
  vehicle's `V+0x204` when that vehicle is the local player (`0x71c298`;
  `if.player_pos_caller_a`, `if.player_pos_caller_b`).

(The other tests of the same evaluator, e.g. the quaternion test behind
`4.0 * |v|² <= event+0x14` at `0x4ec327`, are a different condition and not
`PLAYER_RANGE`.)

## Unresolved

| Unknown | Evidence | Affected content | Resolves in |
| --- | --- | --- | --- |
| Which code path starts the row ANIMATION of a rocket burst | `burst.anim_guard`: the start is skipped when the stack word at `esp+0x44` is non-zero. Under the standard cdecl layout that word is the second argument, the hit record pointer, which every caller passes non-null (it is dereferenced at `0x5ac7a9`), so the block would be unreachable; that contradicts the water splash and every missile fireball, so a layout assumption is wrong somewhere. Not resolved statically in this task. | whether and where `flak_effect` starts at a proximity burst | an owner capture of a flak burst, or a trace that settles the frame layout of `0x5ac7a0` (or finds the other start of the row animation, e.g. through the hit handler `0x5abcf0`) |
| What `[anim+0x6c]` is for an animation started at a position with no node | `0x4ed730` was not followed | whether the `PLAYER_RANGE` distance is measured from the burst point | the animation-start trace above |
| When the `IF` is evaluated (once at the start, or when the sequence reaches it) | only the evaluator was traced | the timing of the flicker relative to the puff | animation sequence runtime trace |
| The runtime of `FBFX_COLOR_FROM_TO` (the flicker itself) | not traced | the six-step flash | F27/FBFX work |
| What the pool does with `rocket+0x20`, and the pointee of wrapper slot 0 | not traced | the exact target point of the 20 m test | weapon-pool trace |
| The names of `[[0x71c298]+0x91d]`, `0x64f66e`, `entry+0x6f`, `entry+0x6c/+0x6d` | FLAK-A, unchanged | dormant/frozen turret cases | FLAK-A table |
| Who consumes `AMMO`; the turret `HEALTH` unit | FLAK-A, unchanged | turret ammunition, hits to silence a gun | FLAK-A table |

## What this settles for FLAK-C2 / C3 / C4

* **C2 (round):** a turret's `wep_27` shot has no lead and no intercept; the
  rocket carries the held target when it is fired; it bursts at 20 m from that
  target (squared distance test), at the end of `RANGE` 900 m regardless of a
  target (`LOCK_ON` without `EXPIRES`), or never silently.
* **C3 (burst effect):** the effect belongs to `IMPACT` row `default`
  (`flak_effect`); the flicker only when the player is within 100 m of the burst
  node. The start path is the unresolved item above: do not claim original
  fidelity for *where/when* the animation starts before it is resolved.
* **C4 (sound):** the shot sound is the `FIRE` row's `snd_enemyflak` (not traced
  here); the burst plays `snd_missile_flak` through the row sound and the
  `0x5ad100` pick.
