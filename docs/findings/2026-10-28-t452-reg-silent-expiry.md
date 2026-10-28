# T452-REG-SILENT-EXPIRY (#2175): the measured end of `RANGE` in the fuse vocabulary

Date: 2026-10-28. Task: #2175, the first of the follow-ups #2171 filed against
the runtime registration (#2156). Spec:
`specs/F28-rockets-special-ordnance-counter-effects-and-nitro.md` (stage
`### F28-D`). Shared contract: `docs/contracts/FLIGHT-PHYSICS.md`, "Boost and
special models". Capabilities used: `retail` (read access to `$CS_GAME_DIR`)
and ordinary build/test; the engine-image facts reused here are
`docs/findings/2026-10-28-t452-r02-ordnance-effects-fuses-and-swarm.md` §3
(static analysis already performed and pinned by T452.R02's own image test).
**Not** used and not claimed: any run of the original executable, any GPU, audio
or human observation.

## Files and the one observable failure (listed before editing)

- `crates/cs_sim/src/weapons/ordnance.rs` (extend): `FuseRule::ExpiresAtRange`,
  `FuseDecision::Consumed`, `OrdnanceState::{is_consumed, consumed_range_m,
  has_ended, reaches_range_end}`, the `fuse_decision` order,
  `OrdnanceRuntime::advance`, and two `OrdnanceDefinitionError` variants.
- `crates/cs_content/src/ordnance.rs` (extend): `DeclaredFuseRule::ExpiresAtRange`,
  its `walk` field and two `OrdnanceSchemaError` variants.
- `crates/cs_app/src/ordnance.rs` (extend): `lower_fuse`, the measured fuse
  selection, `OrdnanceConsumption`, `OrdnanceSessionTick::consumed`,
  `OrdnanceEventKind::Consumed` and the session's consumption arm.
- `crates/cs_sim/tests/accept_t452_silent_range_end.rs` (**new**, 5 tests) and
  `crates/cs_sim/tests/main.rs` (wiring: one `mod`).
- `crates/cs_app/tests/accept_t452_silent_expiry.rs` (**new**, 6 tests: 4
  synthetic, 2 retail), `crates/cs_app/tests/evidence_report_t452_silent_expiry.rs`
  (**new**, evidence harness) and `crates/cs_app/tests/suite_a.rs` (wiring: two
  `mod` declarations).
- `crates/cs_app/tests/accept_t452_register.rs` and
  `crates/cs_app/tests/evidence_report_t452_register.rs` (previous stage's
  fixtures: the `wep_12`-shaped record's silent end is now expressible, and the
  retail expectation moves from ten registered / six blocked to eleven / five).
- This file and `docs/findings/evidence/T452-REG-SILENT-EXPIRY.json`.

No protected path, no `Cargo.toml`/`Cargo.lock` change, no original data, no
binary file, no original display text and no script text.

**One observable failure, before the change:** `#2171` measured which original
components fly and which cannot, and three ids — `wep_12`, `wep_13`, `wep_26` —
were blocked on `t452_register.limit.silent_expiry` because their measured end of
flight is a silent removal at the end of `RANGE`. `FuseRule` had three variants:
`Impact`, `Proximity` and `Timed`, and `Timed` **detonates**. `ArmingRule::Disarmed`
expresses "never detonates" but overstates the measurement: nothing measured says
a collision cannot detonate these records. So no runtime value could say "this
item flies to its record's `RANGE` and is consumed there without detonating", and
`step_ordnance_session` had no way to report that end.

## What the measured end is

T452.R02 §3 measured the unguided projectile step: at the end of `RANGE`
(`0x5b01ec`) it detonates only when `LOCK_ON` (without `EXPIRES`) or the explicit
`DETONATE_AT_RANGE` key is set; otherwise it stores `0x5b0318`'s zero to the
projectile's `+0x20` active word and the sweep unlinks it. For the records this
stage registers that means:

- `wep_13` (area denial, `SMOKE_SCREEN`): no `VELOCITY`, no `RANGE`, no
  `LOCK_ON`, no `DETONATE_AT_RANGE` — so the parser's defaults (T452-R.01:
  `VELOCITY 120.0`, `RANGE 500.0`) and the **silent** end at 500 m. Its smoke is
  a separate emitter created at fire time (`0x4b8d50`): no measured *gameplay
  area radius* reaches it (the emitter's `smokescreen_stun_range` is a stun
  range resolved by `T452-REG-SMOKE-RADIUS`, not the runtime's bounded area), so
  the item itself opens no area. What the field does to the actors inside it
  stays the named claim `t452_r02.limit.smoke_stun_effect`.
- `wep_26` (flak, `ROCKET`, `IMPACT_PROXIMITY 100`, `RANGE 900`): the same silent
  end; its `IMPACT_PROXIMITY` is a blast radius, not a range-end trigger, and the
  proximity branch is gated on `LOCK_ON`, which the record lacks.
- `wep_12` (area denial, `TANGLER`): the same silent end. It stays blocked for a
  **different** measured gap: its tangler field lasts 2 s and lives on its own,
  which the runtime's bounded area (applied once, never outliving its item)
  refuses — follow-up `T452-REG-TANGLER-EFFECT`.

## The runtime shape

`FuseRule::ExpiresAtRange { range_m }` is the fourth variant: a **distance**
bound, not a countdown. `OrdnanceState` latches it in
`FuseDecision::Consumed { range_m }`, a third decision kind beside `Triggered`
and `Inert`, so a caller can tell "the shell flew its range" from "the runtime's
designed cap ran out" (`FuseInert::Expired`) and from "something detonated".

The decision order in `OrdnanceState::fuse_decision` is the contract, and the new
step sits at position 6:

1. already ended (triggered, consumed or retired) → `AlreadyTriggered`;
2. not armed → `NotArmed` (AC01's "not before arming");
3. a swept contact → `FuseTrigger::Impact`;
4. a `Timed` clock → `FuseTrigger::Timed`;
5. a crossed `Proximity` radius → `FuseTrigger::Proximity`;
6. **the measured `RANGE` bound reached → `FuseDecision::Consumed`**;
7. otherwise the named inert reason.

Position 3 before position 6 is deliberate and tested: the measurement covers the
end of `RANGE` and says nothing about a collision, so a contact still detonates a
range-bounded item rather than being hidden behind the silent end.

`OrdnanceRuntime::advance` leaves an item whose measured bound is due to the fuse
decision instead of expiring it, because a measured record's designed lifetime
cap is derived from the same `RANGE` and can fall on the same tick: without that
deferral the report would name the cap and never the measured end. The item is
removed on that tick either way (`session.runtime.remove`), so the deferral never
extends a flight, and `OrdnanceState::reaches_range_end` is the single predicate
both the runtime and the fuse decision answer, so they cannot disagree.

`step_ordnance_session` reports the end as `OrdnanceSessionTick::consumed`
(`OrdnanceConsumption { projectile, range_m }`) and emits
`OrdnanceEventKind::Consumed`, with no `Triggered` event, no detonation, no area
and no routing into `DamageResolver`. The ECS mirror is despawned by the ordinary
mirror pass, because the item is no longer live.

## The lowering

`measured_ordnance_declaration` now picks the fuse in this order, unchanged for
the first two:

| the record's measured end | declared fuse |
| --- | --- |
| a seeker's `DETONATION_DISTANCE` (the measured proximity path needs `LOCK_ON`) | `Proximity` |
| a declared `DETONATION_TIME` | `Timed` |
| no timer, and the measured end of `RANGE` is `ExpiresSilently` | **`ExpiresAtRange { range_m: geometry.range.metres() }`** |
| no timer, and the measured end of `RANGE` detonates | `Timed` at the lifetime cap — a guided record's detonation is its lost-target rule (`CoastThenDetonate`), and no measured record is both unguided and `DETONATE_AT_RANGE` (T548), so that case is not asserted for an unguided record |

The bound is the record's own `RANGE` (`geometry.range`): the declared value, or
the parser's measured `500.0` default for a record that states none. The lifetime
stays a separate designed cap (the record's flight rule at a stationary launcher,
rounded up); it is not what ends these flights, and the tests show the measured
bound ending the flight before the cap falls.

The `t452_register.limit.silent_expiry` claim is no longer raised and no record is
blocked on it. The constant stays, documented as resolved, as a resolved claim id
is kept elsewhere in this crate: the evidence report of the stage that recorded it
names it, and `docs/findings/evidence/T452-RUNTIME-REGISTER.json` is the record of
that commit rather than a statement about this tree.

## What registers now, and what stays blocked

Eleven of the sixteen declared ids register (`wep_13` is new); five stay blocked,
each on its own measured gap, which this stage does not touch:

| id | outcome | why |
| --- | --- | --- |
| `wep_05`, `wep_06`, `wep_07`, `wep_08`, `wep_09`, `wep_10`, `wep_11`, **`wep_13`**, `wep_14`, `wep_15`, `wep_24` | registered | the measured values are expressible; `wep_13`'s silent end is `ExpiresAtRange { 500 }` |
| `wep_04`, `wep_25`, `wep_27` | blocked | `ACCELERATION 150`: the measured lateral-remainder ramp (`t452_register.limit.inheritance_ramp`, `T452-REG-INHERITANCE-RAMP`) |
| `wep_12` | blocked | the tangler field outlives the item that creates it (`t452_r02.limit.tangler_flight_effect`, `T452-REG-TANGLER-EFFECT`) |
| `wep_26` | blocked | the ramp (`t452_register.limit.inheritance_ramp`) — its measured silent launch became expressible in `T452-REG-MEDIA-SILENCE` |

So the task's "then those ids register" is met for the id this claim alone held
back (`wep_13`), and **not** for `wep_12` or `wep_26`: their names stay blocked by
measured gaps with their own filed tasks, and this stage deliberately does not
default anything to make them fly. The retail tests assert exactly that — no id
names the silent-expiry claim, `wep_12` names only the tangler claim, and `wep_26`
keeps the ramp claim (its measured silent launch is expressible now that
`T452-REG-MEDIA-SILENCE` has landed).

The catalogue keeps the claims T452-R.02 left on these records for effects this
stage does not express: what `wep_13`'s smoke field does to the actors inside it
(`t452_r02.limit.smoke_stun_effect`) and the tangler's flight effect
(`t452_r02.limit.tangler_flight_effect`). Neither is resolved, removed or folded
into this stage.

## The acceptance criteria, answered

1. **A range-bounded, non-detonating end in `FuseRule`, handled by
   `OrdnanceState`/`OrdnanceSession`.** `FuseRule::ExpiresAtRange`, latched as
   `FuseDecision::Consumed`, removed by the session and reported as
   `OrdnanceSessionTick::consumed` with an `OrdnanceEventKind::Consumed` event.
2. **Lowered from `OriginalRangeBound` through `cs_app::ordnance`.** The bound is
   `geometry.range.metres()` (`OriginalRangeBound::Declared`/`Defaulted`) carried
   as a `Resolved<f64>` field (`fuse.range_m`) and lowered by `lower_fuse`.
3. **The blocker for those ids is gone.** `t452_register.limit.silent_expiry` is
   raised by nothing; `wep_13` registers and is consumed at its measured bound.
4. **`AreaDenialEngine` still refuses an impact fuse.** Unchanged and re-tested
   (`accept_t452_silent_an_area_denial_component_end_on_range_but_not_on_contact`).
5. **A test with prefix `accept_t452_silent_` drives a registered unguided
   area-denial record through `step_ordnance_session` and shows the item consumed
   at the end of its measured range with no detonation, no area and no damage.**
   `accept_t452_silent_a_registered_area_denial_record_is_consumed_at_its_range`
   does that over the lowered fixture, and
   `accept_t452_silent_retail_the_measured_smoke_record_is_consumed_at_range` does
   the same over the owner's own `wep_13`: registered by
   `register_measured_ordnance`, launched through the session, consumed at 500 m
   on tick 124 of a 126-tick cap, with an empty `detonations`, `guidance_blasts`,
   `areas_expired` and `expired` list on every tick, a supplied blast-recipient
   list that would have taken a hit, and the target's hull integrity unchanged.

## Test sensitivity (measured, one mutation at a time)

Each mutation was applied to the committed production file and the
`accept_t452_silent_` selection re-run (the CI selection, without
`--include-ignored`), then the file restored byte-identically
(`shasum 0bc8e0a64034b258ae4d4e1c696175c4f67efff7` of
`crates/cs_sim/src/weapons/ordnance.rs` and
`c4d563be097c3a4fd1bdeff4f9e83c32f2225eb4` of `crates/cs_app/src/ordnance.rs`):

| mutation | result | caught by |
| --- | --- | --- |
| the runtime's lifetime cap preempts the measured end again (the `reaches_range_end` deferral in `OrdnanceRuntime::advance` disabled) | 1 failed, 4 passed (`cs_sim`) | `accept_t452_silent_the_range_end_is_reported_instead_of_the_lifetime_cap` |
| the silent end lowered as a `Timed` clock at the lifetime cap instead of `ExpiresAtRange` | 3 failed, 1 passed (`cs_app`) | `..._a_measured_area_denial_record_lowers_to_its_range_end`, `..._a_registered_area_denial_record_is_consumed_at_its_range`, `..._the_consumption_follows_the_path_not_the_clock` |
| the measured range end decided **before** the swept contact | 1 failed, 4 passed (`cs_sim`) | `accept_t452_silent_a_contact_still_detonates_a_range_bounded_item` |
| the consumed item left in the live set (the session's `remove` dropped) | 1 failed, 3 passed (`cs_app`) | `..._a_registered_area_denial_record_is_consumed_at_its_range` |

The retail tests are the second half: they re-read the owner's installation
through the production readers on every run, so a changed `RANGE`, a changed
`VELOCITY`, a changed end-of-range gate or a changed registration outcome fails
rather than passes.

## Evidence

`private/evidence/T452-REG-SILENT-EXPIRY/acceptance.json`, committed as
`docs/findings/evidence/T452-REG-SILENT-EXPIRY.json`, validates with
`python3 tools/validate_evidence.py private/evidence/T452-REG-SILENT-EXPIRY/acceptance.json
--artifact-root private/evidence/T452-REG-SILENT-EXPIRY` (structurally: its
`unknowns` carry the seven measured values the registration still cannot express
for other ids, derived from the production report, so `--require-pass` is not used
and no limitation was removed to satisfy a validator). It records `capabilities:
["retail", "synthetic"]`, the `accept_t452_silent_` selection — 6 tests, 6 passed,
0 failed, with `--include-ignored` and `CS_GAME_DIR` set — the installation
digests measured by `cs_assets::install` discovery, and two hashed artifacts: the
recorded `cargo-test.log` and `original-consumption.json`, a second production
pass listing every declared id with its outcome, `wep_13`'s lowered fuse and
lifetime, and two session flights of it (stationary: consumed on tick 124 at
500 m; launcher at 100 m/s along the launch line: consumed on tick 68 at 500 m),
each with zero detonations, zero areas and zero damage events. Identifiers,
numbers and digests only. The report's `candidate_tree` is
`dd66713c1200dcd8084ca53ba72f7f5b0e479d74` (the code-and-test commit's tree);
this file and the committed copy are the documentation commit that follows it,
and nothing the report measured changed.

## Not claimed

No original-run behavior: `retail` here is read access to the owner's files, and
the measurement this stage implements is T452.R02's static analysis of the
owner-supplied decrypted image, never evidence that the original executable ran.
No statement that a consumed projectile is what the original's screen showed, that
its flight took the same wall-clock time, or that a collision behaves as the
original's does — the runtime keeps the contact ahead of the silent end precisely
because that part is unmeasured. Nothing here is `verified_original`, and the
claim the evidence report carries is `implemented`: a code and test pass awards
nothing above that, and no agent review replaces the owner's human approval.

## Review

**Implementer:** `deepseek-1` (agent, model `deepseek-v4.1-flash`), session
2026-10-28. This finding is written by the implementer; the review that follows
the handover is recorded in Rally and in the review notes of the merged commit.
No agent review replaces the owner's human approval, and this stage records
neither `verified_original` nor `release_approved`.
