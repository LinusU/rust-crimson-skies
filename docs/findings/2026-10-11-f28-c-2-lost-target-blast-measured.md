# F28-C.2: the original's lost-target blast, measured from the files and the image

Date: 2026-10-11. Task: #548 "Measure the original lost-target Detonate blast rule
(F28-C.1 follow-up)". Spec: `specs/F28-rockets-special-ordnance-counter-effects-and-nitro.md`,
section `### F28-C` and non-negotiable behavior 1, 3 and 4. Predecessor:
`docs/findings/2026-10-03-f28-c-1-guidance-loss-detonation.md` ("Unknowns recorded").
Capabilities used: `retail` (read access to `$CS_GAME_DIR`, and to the owner-supplied
decrypted executable `$CS_ENGINE_IMAGE`, sha-256
`43540fc97347210d6f4c10b77edbd4cdab1f03d57554d638223c2430a6c37d75`, base
`0x400000`) and ordinary build/test. **Not** used and not claimed: any run of the
original executable. Everything below is what the shipped files and the image
*contain*; none of it is evidence of how the original *plays*, so nothing here is
`verified_original`.

Method: the weapon records were read through the production reader chain
(`cs_formats::zbd` reader archive, `cs_content::original_airframe::SpannedDocument`),
and the image was read with `strings`, Python Capstone 5.0.7 and radare2 (the same
tools as `docs/findings/2026-10-08-m01-lc-campaign-airframe-engine-state.md`).
No disassembly, extracted script or asset is committed; the image is cited by
virtual address and by short byte pins that
`crates/cs_content/tests/accept_t548_retail_blast.rs` re-checks on every run.

## Files and the one observable failure (listed before editing)

- `crates/cs_sim/src/weapons/ordnance.rs` (extend): `AreaEffect::blast_fraction`,
  the `BlastRecipient` record, `OrdnanceRuntime::route_detonation_area` and the
  two refusals `NoAreaEffect` / `NonFiniteRecipient`.
- `crates/cs_sim/src/weapons/mod.rs` (wiring only): the re-export of `BlastRecipient`.
- `crates/cs_content/src/ordnance.rs` (extend): `OriginalRocketBlast`,
  `read_original_rocket_blasts` and the measured-record constants.
- `crates/cs_sim/tests/accept_t548_lost_target_blast.rs` (**new**, 4 tests, run in CI).
- `crates/cs_content/tests/accept_t548_retail_blast.rs` (**new**, 3 `#[ignore]` tests).
- `crates/cs_content/tests/evidence_report_t548.rs` (**new**, evidence harness, not
  an acceptance test) and `docs/findings/evidence/T548.json`.
- This file.

No protected path, no `Cargo.toml`/`Cargo.lock` change, no original data, no binary.
`docs/research/SOURCES.md` is protected and this task is not
`allowProtectedChanges`, so the entries it asks for are **proposed below**, not
written.

**One observable failure, before the change:** F28-C.1 applied a lost-target
blast to exactly one actor — the engagement's named target — at full declared
damage, with no radius and no falloff. The original never does that. A blast
whose declared area contains three actors damaged one of them at full value; the
retail rule damages all three, scaled by distance. After the change
`route_detonation_area` damages every recipient inside the radius by the measured
fraction (test
`accept_t548_a_blast_damages_every_actor_inside_the_radius_with_falloff`).

## Where the rule lives

The original has no "lost-target Detonate" behavior. The records in
`ZBD/zrdr.zbd` / `weapons.zrd` carry no key for it, and the image has no code
path that reacts to a target loss by detonating. What the image does contain, and
what answers the five questions, is three things:

1. **The record parser** (`zwep_init.c`, `0x5ad630`): which record keys exist and
   which field of the in-memory weapon each one fills. `DETONATION_DISTANCE`
   fills the weapon's `+0x44` (stored squared), `IMPACT_PROXIMITY` fills `+0x3c`
   (and its square at `+0x40`), `DETONATION_TIME` `+0x48`, `LOCK_ON` `+0x58` plus
   the lock-on flag (bit 15 of the flag word at `+0x74`), `RANGE` `+0x1c`,
   `ARMOR_DAMAGE` `+0x4c`, `HEALTH_DAMAGE` `+0x50`. The flag bits the rule reads:
   `EXPIRES` bit 7, `MINE` bit 14, `TETHER_GUIDED` bit 21, `DETONATE_AT_RANGE` bit 25.
2. **The projectile step** (`0x5afd50`, weapon in `edi`, projectile in `esi`): the
   detonation decisions, `0x5b01cc`..`0x5b0318`.
3. **The detonation** (`0x5ac3a0`) and its area routine (`0x5aca30` query,
   `0x5acac0` apply).

### What a "lost target" is

Each projectile holds its target as a pointer at projectile offset `+0x70`.
`0x5ac0c0` is a sweep run when an object is removed: it clears that pointer on
every projectile that was aimed at the removed object (byte pin
`0x5ac0ec`). So a "lost target" is **a target pointer of zero**, nothing more.

### What the projectile step then does

Every step, after moving, the step accumulates distance travelled
(projectile `+0x664`) and decides:

- **Still inside `RANGE`**: if the weapon is locked-on (`LOCK_ON` present), the
  target pointer is non-zero and `DETONATION_DISTANCE` is positive, it measures
  the squared distance to the target and detonates when that is not above the
  squared `DETONATION_DISTANCE` (`0x5b0222`..`0x5b0247`). **A zero target pointer
  skips this branch** (byte pin `0x5b021b`). It then falls to the timer: if
  `DETONATION_TIME` is positive and the projectile's age exceeds it, it
  detonates (`0x5b029a`); otherwise the step returns and the projectile flies on.
- **Past `RANGE`**: a locked-on weapon without `EXPIRES` detonates; a
  locked-on weapon with `EXPIRES`, and any non-locked weapon, detonates only with
  `DETONATE_AT_RANGE`. Otherwise it clears the projectile's active word and returns.
  The arithmetic is `0x5b01f9`..`0x5b0211`; what removes the projectile afterwards
  is **not measured**.

All three outcomes call the same detonation `0x5ac3a0` at the projectile's own
position (`+0x48`). A lost target therefore changes **when** a rocket detonates
(timer or end of range instead of proximity), never **what** it does.

## The five facts

### 1. What a lost-target detonation damages — measured

**Every object inside the blast sphere, not the tracked actor.** `0x5ac3a0`
calls the impact-effect routine (`0x5ac7a0`) and then, when the weapon's
`IMPACT_PROXIMITY` is positive, the sphere query `0x5aca30`. The query
(`0x4cb420`, a spatial-grid search) takes the projectile's position, a radius of
`projectile scale (+0x678) x IMPACT_PROXIMITY` (byte pin `0x5aca5b`), and returns
the objects it finds; `0x5acac0` applies damage to each. The target pointer is not
an input to either routine, so a lost target changes nothing about the recipients.

Two details of the apply loop are **not** measured and are recorded unknown: the
loop skips a found object when a field at offset `+0xbc` of its object equals the
caller's fourth argument (`0` for a detonation) — what that field is is
unmeasured — and the global at `0xa1d7d4` (initialised to 1 and cleared around
`0x5ac210`) gates the whole area call; whether it is an authority or
re-entrancy switch is unmeasured.

### 2. The falloff — measured

**`1 - d^2 / r^2` of the weapon's armor and health damage**, where `r` is
`IMPACT_PROXIMITY` (byte pin `0x5acb37`: the entry value divided by the squared
radius, subtracted from 1.0). A `MINE`-flagged weapon (bit 14, pin `0x5acb1b`)
skips the scale and delivers the full damage. The product is then scaled by the
projectile's own `+0x678`. The query's radius test compares squared distances
(it stores `r^2` in a global before the search), which is why the entry value is
read as a squared distance; the **write site of that entry value was not read**,
so "squared" is an inference, not a pin. No retail rocket declares `MINE`.

The Rust model: `AreaEffect::blast_fraction` is `1 - (d/r)^2`, `0.0` at or
beyond `r`. It is unit-free; it does not decide what `r` is in metres.

### 3. Which channels — measured

**The two channels the record declares for the weapon: `ARMOR_DAMAGE`
(`+0x4c`) and `HEALTH_DAMAGE` (`+0x50`).** The retail census
(`accept_t548_retail_rocket_records_declare_one_radius_and_one_channel_pair`)
reads all fifteen `ROCKET` records and pins the union of their top-level keys at
41 names; none is a separate blast channel set, and the only damage keys are the
armor/health pair (or the single `DAMAGE` of the two flash records). A separate
blast channel set does not exist in any shipped record.

### 4. Whether arming gates a lost-target detonation — measured, as an absence

**No arming rule was found in the detonation path.** The proximity test fires on
the first step it is eligible; its only conditions are the lock-on flag, a
non-zero target pointer, a positive `DETONATION_DISTANCE` and being inside
`RANGE`. `RANGE_MINIMUM` (only the torpedo record, `[300, 0]`) sets bit 31 and is
read at `0x5b0193`..`0x5b01c9`, where it drives a call on the projectile's model
node (`0x4cd210`, argument 1) while the distance travelled is at most the first
value — a presentation/visibility toggle by its shape, not a detonation gate,
but **what `0x4cd210` does with that argument is not measured**. Nothing was
found in the collision/impact path either, but that path was not read in full.
Because a lost target never reaches the proximity test, arming is moot for it.

The Rust model keeps `ArmingRule` as a declared option; nothing here changes it.

### 5. Whether the area effect damages, chokes, stalls or marks — measured for damage; the rest stays unknown

The area routine **damages** (armor and health, above). The choke is a
different record shape: the choker (`wep_12`) declares `TANGLER` with its own
`RADIUS 35`, `TIME 2` and `ENGINE_DEAD [5, 13]`, has **no** `IMPACT_PROXIMITY`
(so it never reaches the damage sphere) and `DETONATION_DISTANCE 35`. The flash
(`wep_09`, `wep_15`), sonic (`wep_08`), beeper (`wep_10`, `wep_11`) and smoke
(`wep_13`) records carry their own flag keys; **the code that consumes those keys
was not read**, so what they do (choke, stall, mark, blind) beyond "they are
declared and are not damage" is **still unknown**. This is the same F28-A unknown,
narrowed to the keys named here.

## What the rule means for the Rust model

- `LostTargetBehavior::Detonate` is a **designed** option (F28-A), not an
  original behavior. The original's loss is "the target pointer is cleared; the
  rocket flies on its last aim and detonates at its `DETONATION_TIME` or at the
  end of `RANGE`". F28-C.1's behavior is not weakened; it stays a declared
  option and is now explicitly *not* what the retail records do.
- The blast is area-wide with a squared-ratio falloff, not single-target. That is
  `route_detonation_area`. `route_detonation` (single named target) stays for
  the F28-C.1 behavior.
- `IMPACT_PROXIMITY` is **not** lowered into a declared `AreaEffect` here: its
  unit scale (original world units against metres) is a separate measurement
  (see `docs/findings/2026-10-06-f16-e-original-unit-calibration-landmarks.md`),
  and a metre radius would be a guess. `cs_content::ordnance::OriginalRocketBlast`
  therefore carries the record's numbers in the original's units and nothing is
  invented.

## Still unknown (with evidence and affected content)

| unknown | evidence it is unmeasured | affects |
| --- | --- | --- |
| unit scale of `IMPACT_PROXIMITY` and `DETONATION_DISTANCE` | no unit appears in the record or the parser; calibration is a separate finding | any metre radius for the fifteen rocket records |
| meaning of the projectile `+0x678` multiplier | read in the radius and in the damage product; its writer was not followed | all area damage values |
| the `+0xbc` object field and the `0xa1d7d4` global in the apply loop | neither writer was followed | which objects a blast actually reaches |
| whether the area entry value is a squared distance | the query stores a squared radius but the entry's write site was not read | the exact falloff constant at a given distance |
| what removes a projectile that passes `RANGE` without detonating | only the active-word clear at `0x5b0318` is visible | non-locked weapons without `DETONATE_AT_RANGE` |
| the effect of `SONIC`, `FLASH`, `BEEPER`, `BEEPER_SEEKER`, `TANGLER`, `SMOKE_SCREEN` | consumers not read | F28 non-negotiable 3's choke/stall/mark statuses |
| what the `RANGE_MINIMUM` node call does | `0x4cd210`'s argument semantics not read | the torpedo |

## Proposed `docs/research/SOURCES.md` entries (not written: protected path)

- `weapons.zrd` in `ZBD/zrdr.zbd` — the fifteen `ROCKET` weapon records
  (`wep_04`..`wep_15`, `wep_24`, `wep_26`, `wep_27`); read by
  `cs_content::ordnance::read_original_rocket_blasts`; the blast-relevant keys are
  `IMPACT_PROXIMITY`, `DETONATION_DISTANCE`, `DETONATION_TIME`, `ARMOR_DAMAGE`,
  `HEALTH_DAMAGE`, `LOCK_ON`.
- `crimson.decrypted.exe` (owner-supplied) — `0x5ad630` record parser,
  `0x5afd50` projectile step, `0x5ac3a0` detonation, `0x5aca30`/`0x5acac0` area
  query/apply, `0x5ac0c0` target-pointer clear; re-checked by byte pins in
  `crates/cs_content/tests/accept_t548_retail_blast.rs`.

## Test sensitivity (measured, one mutation at a time)

| mutation | caught by | result |
| --- | --- | --- |
| `blast_fraction` returns `1 - d/r` instead of `1 - (d/r)^2` | `accept_t548_the_measured_falloff_is_one_minus_squared_ratio`, `accept_t548_a_blast_damages_every_actor_inside_the_radius_with_falloff` | 2 failed |

## Not claimed

No original-run behavior, no unit calibration, no wiring of `route_detonation_area`
into `step_ordnance_session` (the session holds no actor list; the producer would
have to supply the recipients — follow-up filed with Rally), and no change to the
declared catalogue. This task awards at most **checked**.
