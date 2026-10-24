# M01 (`ZBD/C3/M01`): what starts `hydrogen_fire` (#2087)

Date: 2026-10-24. Task: #2087, follow-up to #2086's census. Capability: `retail`
(read-only `$CS_GAME_DIR`) and the owner's decrypted image read statically
(`$CS_ENGINE_IMAGE`, disassembly kept in `private/`). Nothing here ran the original
executable; nothing is `verified_original`. Tests: `accept_m01_hydrogen_fire_` in
`crates/cs_app/tests/campaign/m01_hydrogen_fire_trigger.rs` (both
`#[ignore = "requires CS_GAME_DIR"]`).

## Result

The data names the whole chain from the cargo zeppelin's hydrogen tanks to
`hydrogen_fire`, and nothing in the data starts the first link. That link is a
health-driven start whose engine consumer is **not located**: recorded as unknown.

## 1. The measured chain (data, via the production animation reader)

`ZBD/C3/M01/mis_anim.zbd`, records read with `read_animation_index` and
`decode_event_stream`:

| record | anim | object | root | activation | `max_health` | notes |
| --- | --- | --- | --- | --- | --- | --- |
| #361..#364 | `hydrobombboom1..4` | `hydrogentank1..4` | `healthy` | 0 | 15 | see below |
| #365 | `hydrogen_fire` | `hydrogenfire` | `hydrogenfire` | 3 | 0 | call-only effect: `hydrofire` puffer sequence, stops itself at 35 |
| #366 | `cargozep1_crash` | `cargozep1` | | 3 | 0 | called from the tank records' default sequence at +35 |

Each `hydrobombboomN` record's unnamed (default) sequence invalidates itself, activates
`burners`, plays the explosion sounds, calls `large_fireball` / `he_ground_effect`,
calls `hydrogen_fire` four times (`WITH_NODE burn1..burn4`), calls the gasbags
`cg1zepright_gasbag1`, `cg1zepleft_gasbag1`, `cg1zepleft_gasbag5`,
`cg1zepright_gasbag2`, then `CALL_SEQUENCE call_others` and `CALL_ANIMATION
cargozep1_crash`. `call_others` (`ACTIVATION ON_CALL`) calls `hydrobombboom1..4`, so
**the first tank to go starts all four**. The `hydrogen_fire` record is called by exactly
these four records and by nothing else.

The source definition is one `ANIMATION_DEFINITION` in `ZBD/C3/zrdr.zbd` (the group
damage reader; `hydrogen.zrd`, listed in the mission's `mis_anim.zbd` and `zrdr.zbd`):
`NAME1 hydrobombboom*`, parent `cargozep1` `underneath`, selector `hydrogentank*`,
`ANIMATION_ROOT_NAME healthy`, `HEALTH 15`, `RESET_STATE` (`healthy` active, `dbase`
and `burners` inactive). It carries **no** `DAMAGE_SEQUENCE` and no `ACTIVATION`.

## 2. What the data does not contain

* No `CALL_ANIMATION hydrobombboomN` outside those four records (every record of the
  mission's container was decoded; the test pins the caller set).
* The control program (`ZBD/C3/M01/zrdr.zbd`) names `hydrogen_fire` once (OBJECTIVE25's
  `ANIM_STATE`) and `hydrogentank1` as an objective target (OBJECTIVE38 sets it;
  OBJECTIVE25 clears it); no `WAKE_ANIM`, `CALL_ANIMATION` or other statement names a
  `hydrobombboom` record. The bytes `hydrobombboom` occur only in `C3/M01/mis_anim.zbd`
  and `C3/zrdr.zbd`.
* No other animation container of the install holds the name.

So within the data the tank records are started by something that is not a spelled call.
The measured shape that fits is the one shared with the zeppelin `destroy_*` records
(`HEALTH 40`, activation 0, root `healthy`): the animation definition's `HEALTH` is its
trigger, the target being a damageable object (`hydrogentank<N>` is bound as a damageable
world target by `mission_targets`, `2026-10-18-m01-bridge-convoy-spawn.md`).

## 3. Engine side (static, `$CS_ENGINE_IMAGE`)

* The definition parser reads `HEALTH` (string at VA `0x633bdc`, handler `0x51ee90`) into
  the animation object at `+0xb4` and `+0xb8` (integer values are converted with `fild`,
  `0x51eefb`..`0x51ef0a`). `DAMAGE_SEQUENCE` (string `0x633c34`, handler `0x51e7ec`)
  is skipped by the parser (`ebp += 8`; the sequence is not stored there).
* **Not found:** the code that lowers that health and starts the record at zero. A scan
  of every `[reg + 0xb4]` access in `.text` gave the vehicle/script accessors
  (`0x4e7130` returns `[obj+0xb4]` or `0.0f`, callers `0x4a5e59`, `0x4a5ec3`, `0x4bac8f`),
  the puffer interpolation (`0x54fd4c`) and unrelated structures; none of them is the
  animation object's damage step. The animation object's runtime class was not
  identified from the parser's `ebx`.

So the claim "damage to `hydrogentank<N>` taking its `HEALTH 15` to zero starts
`hydrobombboom<N>`'s default sequence" is **plausible and unmeasured**. The pool is in
the record's own units (the weapon `HEALTH_DAMAGE` unit, `splash.health_damage_field`
in `data/owner-static/engine-facts.toml`), the damage-to-animation coupling is the
standing `f20-anim.damage-coupling-unmeasured` claim, and no number was invented.

## 4. What our runtime does today

The host already starts a record on `CALL_ANIMATION` (stage 3b) and reports a running
record to the `ANIM_STATE` condition, so once a `hydrobombboomN` record is started,
its own default sequence would start `hydrogen_fire` and OBJECTIVE25 would see it
running. The missing piece is only the first start. Nothing starts it: no hook couples a
bound target's destruction to its definition's record. The tanks are shootable
(`DESIGNED_TARGET_INTEGRITY` pool) and their destruction starts nothing.

Not implemented: a destruction-to-record hook would guess the trigger law (threshold,
which damage counts, whether the start is on the destroying tick) that AGENTS rule 4
forbids guessing. Follow-up filed.

## 5. Effect on the win route

Block 25 (and therefore 26 and the OBJECTIVE13 gate) stays unreachable. Everything the
route needs from this finding: destroy a `hydrogentank<N>` (target `hydrogentank1` is the
one the HUD marks; group 1 is the Medusas), after block 23 woke 25. The
`cargozep1` is woken by block 39, 2 s after block 23.

## Unmeasured, with affected content and resolver

| Unknown | Affects | Resolver |
| --- | --- | --- |
| Engine code that lowers an animation object's `HEALTH` and starts its default sequence | the win of "The Lost Treasure"; also `destroy_*` zeppelin part records | follow-up: static trace of the animation object's runtime class (parser `0x51e7ec..0x51fb03` builds it) |
| Which hits count against the tank's pool (guns, rockets, bombs, splash) | how the player must hit the tank | same trace |
