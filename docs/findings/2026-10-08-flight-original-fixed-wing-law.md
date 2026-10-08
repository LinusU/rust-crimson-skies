# Task #796: the original fixed-wing flight law and its retail parameters

Date: 2026-10-08. Task: #796 "Implement the original fixed-wing flight law and
import per-airframe parameters from vehicle.zrd/engines.zrd/player.zrd"
(key `FLIGHT-ORIGINAL-MODEL`, priority 100, `allowProtectedChanges: false`).
Provenance label: **`OWNER-STATIC-2026-10-08`**. Capabilities used: `retail`
(read-only access to `$CS_GAME_DIR`) plus ordinary build/test. Test prefix:
`accept_flight_original_`.

## What `OWNER-STATIC-2026-10-08` is, and what it is not

Every equation, constant and default below was recovered by owner-requested
static analysis of the owner's decrypted image
`crimson.decrypted.exe`, sha256
`43540fc97347210d6f4c10b77edbd4cdab1f03d57554d638223c2430a6c37d75`, image base
`0x400000`. The owner accepts this label in place of an original run *for this
purpose*.

It is **static code evidence**: no original executable ran, nothing here is an
observed behaviour, and nothing is marked `verified_original`. The retail test
asserts that every imported value's provenance class is *not*
`verified_original`. The parameter *values* are not hard-coded anywhere in the
code: they live in retail data files and are imported (below).

## What was built

| File | Contents |
| --- | --- |
| `crates/cs_sim/src/flight/original.rs` | The law: atmosphere, thrust, drag, velocity-steering lift, the rotation terms, the kinematic integration, the fake-dynamics branch, and the flat parameter vocabulary (`AIRFRAME_FIELDS`, `GLOBAL_FIELDS`) |
| `crates/cs_sim/src/flight/tuning.rs` | `ModelKind::OriginalFixedWing`, a third model kind rather than a profile of the designed law |
| `crates/cs_sim/src/probes/runner.rs` | Wiring only: the exhaustive `ModelKind` match now refuses the new kind by name |
| `crates/cs_content/src/original_airframe.rs` | The importer: reads the three members through `cs_formats::zbd` + `cs_content::stunts::decode_zrd`, resolves `kind_of`, records a span and provenance per value |
| `crates/cs_content/tests/accept_flight_original_retail.rs` | The retail table assertions and the headless probe |

## The law, with the addresses it came from

One step per frame, `dt <= 0.125 s`, no substeps:

1. **Throttle** slews toward the command at `0.5/s`; a player burns
   `dt * throttle * 5` fuel and freezes at `fuel <= 0`.
2. **Atmosphere** (`0x41aca0`), `alt_ft = y * 3.2808399`: at or below
   `6561.68 ft` (`0x463640` sets the `2000 m` switch) `k = 0.9884208`,
   `rho = 0.9544815 * 0.002377`; above it, and this is a **hard ceiling** with
   no interpolation, `k = 0.7348`, `rho = 0.0570481 * 0.002377`.
   `a_ft = (k + 1) * 558`, `q = 0.5 * rho * v_ft^2`, `M = v / (a_ft * 0.3048)`
   (the same number as `v_ft / a_ft`: `3.2808399` and `0.3048` are
   reciprocals).
3. **Thrust** (`0x41acf0`), `M' = max(M, 0.1)`:
   `Tc = 0.73 * (0.12 - M'/60) * 0.5*rho*((0.84*M' + 0.112)*a_ft)^2 /
   (M' * (1.33k)^(1.41*M'))`, then
   `T = Tc * throttle * engine * S * (z.y <= 0 ? 1 + 0.13 z.y : 1) *
   (1 + 0.24 z.y)` along the nose, where `z` is the body **+Z** axis in world
   space, so a level attitude gives `x1` and a vertical climb gives
   `0.87 * 0.76 = 0.6612`. Nitro replaces the throttle with `1.8` and scales
   drag by `0.8`; engine-out sets `T = 0`.
4. **Drag** (`0x41ada0`): `CD = 0.73 * (0.12 + 0.8M + 0.5M^2)`,
   `D = q * S * drag_factor * CD` against the velocity and only above
   `0.1 m/s`. There is no induced drag.
5. **Lift is velocity steering** (`0x48c4ba`), not `CL(alpha)`: the target
   velocity blends from the current velocity to `|v| * nose` across
   `liftAOAs = 5, 9 deg` (players; AI always aims at the nose), a_req is
   `lift_accel_rate * (vt - v) + (0, g, 0)`, its X/Y projection becomes the
   load command `Gcmd = |(ax, ay)| / 9.82` (0 below `2.4384 m/s`, direction
   `(0.7, 0.7)` when the projection vanishes), and
   `CL = min(clamp(clamp(Gcmd, -5, 9) * W / (qS), -1.8, 1.8), 0.75 - 0.15M)`.
   The load factor is `n = qS * CL * n_y / W`.
6. **Gravity**: `force.y -= (g / 9.82) * W`, i.e. `-g` in acceleration with
   `g = 20`.
7. **Rotation**: every term below is already `* dt` and is accumulated in
   world space into `delta L`. Speed factors `fR` (roll/pitch, ramped
   `turn_fade_in` 10 mph -> `turn_fade_out` 50 mph) and `fY` (rudder:
   `yaw_low_speed` below 10 mph, rising linearly to 1.0 at `yaw_max` 50 mph,
   falling linearly to `yaw_high_speed` at `yaw_fade_out` 400 mph). The
   authority limit `lambda` is `(cos a - cos 46 deg) / (1 - cos 46 deg)`,
   floored at zero, narrowed by `highGs = 9, 15` above `n = 9` as
   `(15 - n)/6` and by `lowGs = -6, -9` below `n = -6` as `(-9 - n)/(-3)`.
   Terms: roll about Z; pitch about X and yaw about Y, each scaled by `lambda`
   when it would push `alpha` further; the bank coupling
   `fall_off * X.y` about Y (`0x6289f8`) and
   `(bank_off * |X.y| - fall_off * Y.y when Y.y < 0)` about X (`0x6289fc`);
   the player weathervane `return_rate * dt` times the half-angle rotation
   from nose to velocity; the Level-Off assist (Shift+L, command 47, hands off
   only) `level_off_rate * dt` times the half-angle rotation from body Y to
   world up, scaled by `lambda` when it would push `alpha` further; and the
   stall, `s = 1 - qS * min(1.8, CLmax) / W`, which strips any nose-up
   component of `delta L` and adds `stall_mag * s * dt` nose-down.
   Then `L = (L + delta L) * exp(-damp * dt)` and
   `omega = R^T * diag(recI) * R * L`.
8. **Integration**: the attitude rotates by **`2 * |omega| * dt`** per step —
   the original's quaternion exponential does not halve the angle — then
   `v += a * dt`, `pos += v * dt` (semi-implicit), with a `4.47 m/s` floor on
   the nose-ward speed for AI only.
9. **Fake dynamics** (`is_autogyro` or far AI):
   `a = (-fd_speed * throttle * Z) - v`.

Hard-coded constants, each with its address: `fall_off = 0.205` (`0x6289f8`),
`bank_off = 0.165` (`0x6289fc`), `level_off_rate = 4.0` from the image's
default table (`0x478a00`; no `vehicle.zrd` record has the key), the two-layer
atmosphere above (`0x41aca0`), the thrust and drag coefficients (`0x41acf0`,
`0x41ada0`), the lift-steering block (`0x48c4ba`), the `9.82` force scale
(`0x48ff88`), the dev-tool probe `0x491c60` and the `player.zrd` lookup
`0x59ddb0`.

## The retail data

All three members come from `ZBD/zrdr.zbd` (installation spelling) through
`cs_formats::zbd::dispatch` -> `read_version_one_index` ->
`read_reader_archive`, then `cs_content::stunts::decode_zrd`, which the
importer runs *beside* its own span-recording walk and refuses the member if
the two trees differ (`CrossCheck`).

Measured on the owner's installation (`vehicle.zrd` at 1397861 + 97917 bytes,
`engines.zrd` at 217960 + 1779, `player.zrd` **first** directory entry at
938413 + 3414):

* `vehicle.zrd` is a root list holding one list alternating record names and
  bodies in file order; 75 records, 24 of them with a `dynamics` block, and
  every one of those 24 carries the same 10 keys (`pitch_torque`,
  `roll_torque`, `rudder_torque`, `return_rate`, `ang_momentum_damp`,
  `rec_moments_inertia`, `fd_speed`, `drag_factor`, `veh_weight`,
  `ref_area`). A `kind_of` naming an **earlier** record copies it (`0x477b70`),
  otherwise the record starts from the image defaults (`0x478a00`), and the
  record's own keys overlay the result (`0x479240`). No retail record names a
  parent that is not earlier and none has a partial `dynamics` block, so the
  "merge nested keys" and "replace the whole block" readings of the overlay
  are indistinguishable on retail bytes; the importer merges leaf paths and
  the synthetic test pins that reading.
* `engines.zrd` is 38 rows of `(id "name" factor)`; id 11 is `Bloodhawk
  Lvl-2` -> `0.62`, id 23 is `Devastator Lvl-2` -> `0.65`.
* `player.zrd` appears **twice** (`0x59ddb0` takes the first match): entry
  #22, 3414 bytes, holds the global block parsed by `0x4735b0` —
  `nom_gravity 20`, `liftAOAs 5, 9`, `maxAOA 46`, `highGs 9, 15`,
  `lowGs -6, -9`, `lift_accel_rate 0.75`, `stall_mag 1.25`,
  `turn_fade_in/out 10, 50`, `yaw_low_speed 0.0625`, `yaw_high_speed 0.17`,
  `yaw_fade_in/max/fade_out 10, 50, 400`, and the two keys the law never
  reads, `drag_factor 1.5` and `drag_fade_speed 40`. Entry #100 (34711 bytes)
  is the unreachable animation document and is never looked up.

Imported retail values (production code, `accept_flight_original_retail_...`):

| record | chain | engine | pitch/roll/rudder | return | damp | recI | fd_speed | drag | W | S | gravity |
| --- | --- | --- | --- | --- | --- | --- | --- | --- | --- | --- | --- |
| `pbloodhawk` | `basic_airplane -> player_airplane -> pbloodhawk` | 11, 0.62 | 3.3 / 7.5 / 2.0 | 3.0 | 5.0 | 1.18, 1.0, 1.1 | 135 | 0.37 | 1900 | 330 | 20 |
| `pdevastator` | `basic_airplane -> player_airplane -> pdevastator` | 23, 0.65 | 3.3 / 6.8 / 2.0 | 3.0 | 5.0 | 0.85, 1.0, 0.8 | 113 | 0.62 | 2850 | 515 | 20 |

`mode "jet"` is inherited from `basic_airplane`, `fuel 54926` from
`player_airplane`, and `level_off_rate 4.0` is the image default above, whose
provenance class is `Documented` with claim
`f796.default.level_off_rate.0x478a00` rather than a retail span.

## Verification, and what it is worth

Hand arithmetic from the formulas above predicts every probe number the task
states, which is what makes the implementation checkable rather than merely
tuned:

* **Level top speed.** At 134.3 m/s under the low layer, `M = 0.39713`,
  `Tc = 49.58` -> thrust `49.58 * 0.62 * 330 = 10144 lb`; `q = 220.2`,
  `CD = 0.37709` -> drag `220.2 * 330 * 0.37 * 0.37709 = 10138 lb`. Balance
  within 0.1 %, and the measured run settles at the value the test asserts.
* **Vertical climb from rest at 500 m.** `Tc = 44.59` ->
  `T = 44.59 * 0.62 * 330 * 0.6612 = 6032 lb` ->
  `a = 6032 * 9.82 / 1900 = 31.18`, minus `g = 20` -> **+11.18 m/s^2**
  (the test asserts 11.2 +/-5 %). The climb settles where thrust x 0.6612
  still balances gravity and drag: at 76 m/s, `T = 5918 lb`, `D = 2043 lb`,
  `(5918 - 2043) * 9.82 / 1900 = 20.03` -> net **+0.03 m/s^2** (the test
  asserts 76 +/-2 %).
* **Above the ceiling.** At 2500 m from rest, `Tc = 2.115` ->
  `T/W = 2.115 * 0.62 * 330 / 1900 = 0.228` (the test asserts about 0.23),
  and with the climb's `0.6612` orientation factor the thrust is 15 % of the
  weight, so the climb decelerates hard.

Measured by the acceptance tests on `$CS_GAME_DIR`, all green:

* `accept_flight_original_retail_bloodhawk_and_devastator_match_the_table`:
  every value above, both engine factors, `gravity = 20` from `player.zrd`
  entry #22, `fuel = 54926`, and `drag_factor` / `drag_fade_speed` recorded in
  the `unused` list with their spans.
* `accept_flight_original_retail_level_top_speed_matches_fd_speed`: the held
  Bloodhawk settles at **134.3 m/s +/-1 %**, and every player fighter
  (`pbloodhawk`, `pdevastator`, `pfirebrand`, `pbrigand`, `pfury`, `pavenger`,
  `pkestrel`, `ppeacemaker`, `pwarhawk`) settles within **1.5 % of its own
  `fd_speed`** — measured errors 0.19 % to 0.95 %. The autogyro, flying the
  fake-dynamics branch, converges to `fd_speed` by construction.
* `accept_flight_original_retail_vertical_climb_and_ceiling`: +11.2 m/s^2,
  altitude gained, settled climb speed 76 m/s, T/W about 0.23 and a losing
  climb above the ceiling.

These are **model checks, not original-run evidence**: they show the law as
implemented matches the owner's static analysis and the retail numbers, not
that the original executable behaved this way.

## Deviations from `docs/contracts/FLIGHT-PHYSICS.md`, and why

The contract says forces and torques go out and Avian integrates; it also says
exceptional airframes may use a different control law, and the task directs a
documented model variant rather than bending the designed `CL(alpha)` law.
`ModelKind::OriginalFixedWing` is that variant:

* **Linear**: the law integrates velocity itself. A consumer that hands motion
  to a rigid body takes `world_force = (W / 9.82) * a`, i.e. the task's
  consistent mass `m = W / 9.82`, and **disables that body's gravity and
  drag**, because gravity (step 6) and drag (step 4) are already inside `a`.
  Applying both would double-count them, which the contract forbids.
* **Angular**: `2 * |omega| * dt` and the discrete `exp(-damp * dt)` cannot
  both be reproduced by a torque-driven rigid body, so this law owns attitude
  while the kind is active. `world_torque = delta L / dt` is reported exactly
  (it *is* the step's momentum change over `dt`) for instrumentation; a
  consumer must not integrate it a second time.
* The law's `dt` is `<= 0.125 s` with no substeps, and `OriginalInput` clamps
  the human controls to their declared ranges as the contract requires.
* The probe's `hold_attitude` flag reproduces the dev tool `0x491c60`
  (attitude held, full throttle). Production flight leaves it `false`.

## Ambiguities in the recovered description, and how they were read

Each of these is a reading of the owner's written analysis, not a new
measurement; a different reading would be a one-line change:

1. **Bank coupling parse.** "(0.165*|X.y| - 0.205*Y.y if Y.y<0)" is read as
   subtracting `fall_off * Y.y` only when `Y.y < 0`, so the upright aircraft
   still gets `bank_off * |X.y|` about X. The alternative reading (the whole
   parenthesis gated by `Y.y < 0`) removes that term when upright.
2. **`lambda` floor.** The floor at zero is stated only for the
   angle-of-attack term. The G-window ratios are folded in with `min` and the
   result is floored at zero as well: a negative authority would reverse the
   pilot's input instead of limiting it. `n > 15` and `n < -9` therefore give
   `lambda = 0`, not a reversed torque.
3. **"Half-angle rotation vector"** is `axis * (angle / 2)`, the vector part
   of the corresponding quaternion, not `axis * sin(angle / 2)`. They differ
   only beyond small angles; the weathervane and Level-Off terms are the only
   consumers.
4. **Drag floor units.** `|v| > 0.1` is read as `0.1 m/s` in the law's own
   velocity units (the original's internal feet would make it `0.1 ft/s`;
   either way it only decides whether a near-stationary body feels drag).
5. **AI speed floor.** "the forward speed is floored at 4.47 m/s" is read as
   the nose-ward component, applied to AI only, after the integration.
6. **`is_autogyro` and "far AI"** both select the fake-dynamics branch, but
   the distance that makes an AI aircraft "far" was not recovered, so the
   branch is a declared `DynamicsKind` the runtime chooses;
   `vehicle.zrd`'s `is_autogyro` selects it in the importer.

## Recorded unknowns (not invented)

From the task, still unresolved here:

* Player spawn speed.
* Campaign loadout engine override (loadout+0x10c, `0x47d4f7`).
* Nitro consumption (nitro's thrust and drag *effects* are implemented).
* AI control law `0x48c220`.
* Joystick scaling and roll sign.
* Ground collision (the probe can end below `y = 0`; nothing stops it).

Added while implementing:

* **Spawn fuel for a record whose chain does not state `fuel`.** AI records
  (`bloodhawk`, `devastator`, ...) inherit only `basic_airplane`, which has no
  `fuel` key, so their `initial_fuel` is an explicit `Resolved::Unknown` with
  a reason, never a zero. The player chain's `54926` is imported.
* **`pbalmoral`'s held level top speed is 56.3 m/s against `fd_speed` 79
  (29 %).** Every other fighter matches within 1 %. The Balmoral is a bomber
  (engine `Balmoral bomber Lvl-2`, `W = 4125`, `S = 1100`, `drag_factor 1.7`)
  and the static analysis does not explain the gap, so it is excluded from the
  fighter check and recorded here rather than fitted away.
* **`high_speed_pitch_fade` (1000 / 1001 mph)** is read by the original but
  its fade direction was not recovered and it is unreachable below `447 m/s`
  in any probe, so the law does not consume it; it is not in the importer's
  vocabulary at all.
* The original's fuel *capacity* semantics (is `54926` a capacity or a
  starting amount) were not recovered; the law consumes
  `dt * throttle * 5` from whatever the state holds.

## Mutation probe

One mutation was run end to end and restored: replacing the thrust
orientation factor with `1.0` in `crates/cs_sim/src/flight/original.rs` made
`accept_flight_original_retail_vertical_climb_and_ceiling` fail with "the
initial vertical acceleration is 27.15279370565527 m/s^2, expected 11.2
+/-5 %", which is exactly the value the factor predicts. The other five
behaviours are pinned by assertions that quote the expected number literally,
so removing them fails without a run: the attitude integration
(`accept_flight_original_rotation_is_twice_omega_dt` asserts the rotation is
`2 * |omega| * dt` *and* that it is not the halved one), the `kind_of` overlay
(the synthetic test asserts `2.4` inherited and `1900` overlaid with their
source records), the engine lookup (the retail test asserts ids 11/23, names
and factors 0.62/0.65) and the lift blend (the unit test compares all three
components of the 7 degree target against the hand-computed blend).

## Tests

Prefix `accept_flight_original_`, all green locally with `CS_GAME_DIR`:

| Test | Location | Needs `CS_GAME_DIR` |
| --- | --- | --- |
| `accept_flight_original_atmosphere_step_at_2000_m_matches_the_formula` | `cs_sim/src/flight/original.rs` | no |
| `accept_flight_original_thrust_coefficient_matches_the_formula` | same | no |
| `accept_flight_original_drag_and_lift_coefficients_match_the_formulas` | same | no |
| `accept_flight_original_lift_steering_blend_at_3_7_and_12_degrees` | same | no |
| `accept_flight_original_authority_limit_narrows_with_attack_and_load` | same | no |
| `accept_flight_original_rotation_is_twice_omega_dt` | same | no |
| `accept_flight_original_steady_rates_match_the_torque_damping_rule` | same | no |
| `accept_flight_original_step_bounds_and_fake_dynamics` | same | no |
| `accept_flight_original_parameter_records_are_refused_by_name` | same | no |
| `accept_flight_original_synthetic_vehicle_zrd_is_kind_of_copy_then_overlay` | `cs_content/src/original_airframe.rs` | no |
| `accept_flight_original_unresolvable_records_are_refused` | same | no |
| `accept_flight_original_field_vocabulary_matches_between_the_two_crates` | `cs_content/tests/accept_flight_original_retail.rs` | no |
| `accept_flight_original_retail_bloodhawk_and_devastator_match_the_table` | same | yes |
| `accept_flight_original_retail_level_top_speed_matches_fd_speed` | same | yes |
| `accept_flight_original_retail_vertical_climb_and_ceiling` | same | yes |

