# The animation `HEALTH` consumer: what lowers it and what starts a tank record (#2088)

Date: 2026-10-24. Follow-up to `2026-10-24-m01-hydrogen-fire-trigger.md` (#2087), which left
"what starts `hydrobombboom<N>`" unmeasured. Capability: `retail` plus the owner's decrypted
image read statically (`$CS_ENGINE_IMAGE`; disassembly stays in `private/`). Nothing here ran
the original executable; nothing is `verified_original`. Cited rows:
`[[animation_health_fact]]` in `data/owner-static/engine-facts.toml` (provenance `static`).
Tests: prefix `accept_m01_hydrogen_fire_hook_` in
`crates/cs_app/tests/campaign/m01_hydrogen_fire_hook.rs`.

## Result

The consumer is located. An `ANIMATION_DEFINITION` is an object of a runtime array
(`[0x9fd14c]`, stride `0x110`, count word `[0x9fd14a]`); its **`HEALTH` pool is lowered by
weapon hits on its target node and the object's default sequence is started when the pool
reaches zero**. The earlier finding's "not found" came from reading the `+0xb4` accessors
(`0x4e7130`) as vehicle code: they are this object's, and the damage step is the sibling
`0x4e7220`.

## The object (parser `0x51df00..0x51fb03`, allocator `0x51fc20`)

| field | meaning | evidence |
| --- | --- | --- |
| `+0xa1` | activation: 0 weapon hit, 1 `COLLIDE_HIT`, 2 `WEAPON_OR_COLLIDE_HIT`, 3 `ON_CALL`, 4 `ON_STARTUP` | parser `0x51e291..0x51e390` (strings `0x633de8`, `0x633dd0`, `0x633728`, `0x633dc4`) |
| `+0xb4` / `+0xb8` | `HEALTH`: maximum / current | `0x51eee5`, `0x51ef0a` |
| `+0xbc` | `ARMOR` (string `0x633d40`) | `0x51e425` |
| `+0x6c` | target node | `0x5232a6` |

A new object is zero-filled (`rep stosd`, `0x51fc5c`), so a definition that states no
`ACTIVATION` is **activation 0, the weapon-hit mode**, with `ARMOR` 0. The `hydrobombboom<N>`
definition (no `ACTIVATION`, no `ARMOR`, `HEALTH 15`) is exactly that.

## The law (handler `0x4e7220`)

1. **Registration** (`0x5231e2` walk): activation 0 registers `0x4e7220` as the weapon-hit
   handler of the target node (`0x5abbf0`); activations 1 and 2 register `0x4e7150` as the
   collide handler. Bit 22 of `+0x9c` (set at the start of every definition, `0x51df1e`)
   applies it to the node's children too.
2. **Dispatch**: the hit dispatcher `0x5abcf0` (callers `0x44323d`, `0x48d55b`, `0x5ac4cd`,
   `0x5ac80f`, `0x5acc24`, `0x5ad01b`, `0x5b13d8`, `0x5b14bd`: the impact and
   detonation paths) calls `[handler+4]([handler], weapon, record, damage_pair)` (`0x5abe62`).
3. **Filter**: a weapon whose flag word has `0x400` (set by the weapon key `FREEZE`,
   `0x5ae321`) returns at once (`0x4e7224`).
4. **Amount**: `damage_pair[1] - ARMOR` (`0x4e724a`); `damage_pair[1]` is the health channel
   (the pair is armor, health, `splash.victim_*_pool` rows of the rocket-splash finding). The
   result is stored back into the pair.
5. **Subtract**: only when the amount is greater than 0.0: `current -= amount` (`0x4e7263`),
   no clamp at zero.
6. **Start**: after every counted hit (damaging or not) `current <= 0.0` calls
   `0x4ed730(object, ten zero arguments)` (`0x4e729f`), which looks the object up
   (`0x4ed8c0`) and starts it (`0x525570`): the **default (unnamed) sequence at zero**. A
   `DAMAGE_SEQUENCE` object (`+0xd4`) is started by `0x4e71e0` on every counted hit; the
   hydrobombboom definition has none.

So the answer to #2088's questions: **every hit that reaches the dispatcher counts** (its health
channel; which weapon kinds reach it is not traced per caller), **the threshold is the record's `HEALTH`** (15, in `HEALTH_DAMAGE` units
after `ARMOR`), and the start is **on the hit that takes the pool to zero or below**, in the
same dispatch.

## What the runtime does now

* `crates/cs_app/src/mission_animation_health.rs`: `AnimationHealthLedger` arms one pool per
  mission-carrier record with activation 0, positive `max_health` and an object that is a
  bound ground target (in M01 exactly `hydrobombboom1..4` on `hydrogentank1..4`), counts the
  `DamageChannel::Internal` amount of each hit on the target's damage node, and asks the
  record player to start the record once, on the hit that empties the pool.
* `damage::AcceptedHitTap`: opt-in resource the damage schedule appends the raw accepted hits
  to (the resolver's own pool is the designed single unit, which clamps and is gone after the
  first hit). `MissionHost::step` stage 3c drains it.
* Tests pin the arithmetic on authored records (CI) and the retail path: three health hits of
  6 resolved through the session's own damage authority leave the tank's pool at 9, 3, -3 and
  the start is made on the third.

## Not done, with affected content and resolver

| Gap | Affects | Resolver |
| --- | --- | --- |
| **The record player refuses `hydrobombboom<N>`** (`PlayRefusal::Undeclared`): the join resolves an animation through `ANIMATION_NAME`, and the group damage definition spells the records only as the wildcard state name `hydrobombboom*` in `NAME1`. So the start is asked for and refused; `hydrogen_fire` does not run. | the win of "The Lost Treasure" (#2083); also every `destroy_*` / group damage record | #2095: declare wildcard `NAME1` state names in the animation join |
| **The tanks are not where the zeppelin is.** `hydrogentank1..4` bind as ground targets at their composed position in the static container (`(15, -67, -70)` for tank 1), not under `cargozep1` (placed at `(-6288, 300, -7701)`). Nothing the player flies at can hit them. | the same | #2096: world-actor child targets follow the actor's pose |
| Repeat starts after the pool is empty (the engine calls `0x4ed730` on every later hit; whether `0x4ed8c0` restarts a finished object is not traced) | a second hit after `hydrobombboomN` ends | trace `0x4ed8c0` |
| `ARMOR` is taken as 0; the definition reader does not surface it | definitions that state `ARMOR` | add the field to the reader when one is armed |
| `DAMAGE_SEQUENCE` (`0x4e71e0`), collide activations (`0x4e7150`), the `FREEZE` weapon filter | other definitions | out of this slice |
| Zeppelin part records (`destroy_*`, `HEALTH 40`) share the shape but the parts are not ground targets | zeppelin parts | separate consumer |

Unrelated, found on the way: `accept_m01_convoy_destruction_ground_box_overlaps_are_the_recorded_ones` fails on `main` as well (#2097).
