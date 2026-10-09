# Task #797: flying the retail playtest on the original flight law

Date: 2026-10-09. Task: #797 "Fly the retail playtest Bloodhawk on the original
flight law and its imported parameters" (key `FLIGHT-ORIGINAL-PLAYTEST`,
priority 100, `allowProtectedChanges: false`). Provenance label:
**`OWNER-STATIC-2026-10-08`** (inherited from #796; static evidence, never
`verified_original`). Capabilities used: `retail` (read-only `$CS_GAME_DIR`) and
ordinary build/test. Test prefix: `accept_flight_original_playtest_`.

## What changed

| file | what |
| --- | --- |
| `crates/cs_app/src/playtest/retail.rs` | `RetailFlight`: the production import of `pbloodhawk` + the globals (`cs_content::original_airframe`), the declared start speed, the `playtest flight` line; `install` now refuses an installation whose **flight** parameters cannot be read, before any geometry spawns |
| `crates/cs_app/src/playtest/scene.rs` | `PlaytestOriginalFlight` (the law record on the body) and a separate retail spawn: one dynamic body whose declared mass is `W / 9.82`, no `FlightAircraft`; the synthetic branch still spawns `synthetic_fixed_wing()` unchanged |
| `crates/cs_app/src/playtest/mod.rs` | `drive_original_flight` (one fixed tick: seed → step → force → attitude), the held command now reaches the law record, `PlaytestError::Flight`, `PlaytestState::original_step_errors`, the `playtest flight` line, module docs |
| `crates/cs_app/src/playtest/command.rs` | comments: `CRUISE_THROTTLE` and `KEY_DEFLECTION` are no longer described as synthetic-only |
| `crates/cs_app/src/playtest/propeller.rs` | **wiring only**: `spin_propellers` reads the engine state from the record the body carries — `FlightAircraft` (synthetic scene) or `PlaytestOriginalFlight::engine()` (retail scene) |
| `crates/cs_app/src/playtest_retail.rs` | the label carries the flight law and its calibration status; `SPAWN_FRACTION_X` `-0.6` → `-0.35` (measured reason below) |
| `crates/cs_app/src/cli.rs` | `--help` states the flight law for `--playtest --cs-path` |
| `docs/PLAYTEST.md`, `docs/PLAYTEST-RETAIL.md` | the label, the law, the imported record, the start speed, the Level-Off gap, the retuned spawn |
| `crates/cs_app/tests/playtest_retail_launch.rs` | the four `accept_flight_original_playtest_` tests (folded into this binary for the runner-disk reason #666/#709/#710/#753/#795/#794 used); the spawn-pose assertion now reads the pose on the first frame, because the retail start speed is no longer the synthetic 55 m/s cruise |
| `crates/cs_app/tests/playtest_retail.rs` | the label pin (verbatim, extended) |
| `crates/cs_app/tests/evidence_report_flight_original_playtest.rs` | the evidence harness |

Wiring edits outside the task's named code list, each one forced by a consumer
that would otherwise have broken: `propeller.rs` (the disc's engine state) and
`cli.rs` (help text quoting the label). No logic lives in them beyond that.

## The consumer mapping (one integrator, one gravity)

`docs/contracts/FLIGHT-PHYSICS.md` sends forces out and lets Avian integrate;
#796 recorded this law as the documented exception and wrote down exactly what a
consumer must do. `drive_original_flight` does only that:

1. **Seed** `OriginalState` from the body's authoritative `Position`,
   `Rotation` and `LinearVelocity` each fixed tick, so a contact the solver
   resolved is what the next step flies from. The body owns the *linear* pose —
   that is the one integrator, and it is why a collision still stops the
   aircraft (measured: the retail smoke reports obstacle contacts again).
2. **Hand the acceleration back** as this tick's `ForceRequest`,
   `world_force = (W / 9.82) * a`, with the body's declared mass exactly
   `W / 9.82` (193.48 kg for `pbloodhawk`'s `W = 1900`) and `NoAutoMass` bound
   after the collider so the derived density cannot replace it. Global gravity
   stays `ZERO` and the body has no drag: both are already inside `a`.
3. **Write the attitude the step integrated** and zero the body's angular
   velocity. The original rotates by `2 * |omega| * dt`, which no torque-driven
   rigid body reproduces, so while this kind is active the law owns attitude and
   the body integrates **no torque at all** (`world_torque` is never submitted).
   Zeroing `AngularVelocity` is what stops a contact's angular kick from being
   integrated a second time on the next tick.

A refused step (a non-finite state) increments `PlaytestState::original_step_errors`
instead of being flown; every acceptance run reports `0`.

## Decisions, and what each one is not

* **Start speed = 55 m/s, declared** (`retail::RETAIL_START_SPEED_M_S`). The
  original's player spawn speed was never recovered (#796 open unknown,
  unresolved until #358), so *no* start speed can be a claim about the original;
  the playtest's own start value is what the scripted free flight was written
  for. What the record decides is the **cruise**: `fd_speed = 135`, measured at
  **134.3216** m/s under full throttle (134 ±2 %) — data, not a choice.
* **Roll is negated into the law.** `FlightInput::roll` is positive
  *right-wing-down* (the F22 contract, `E`), while the recovered law integrates a
  positive roll input as `delta L` about body **+Z**, which is right-wing-*up*:
  measured with the direct mapping, holding `E` banked the retail aircraft
  **−47.19°** and the scripted roll checks failed. The original's own roll sign
  is one of #796's open unknowns ("roll sign"), so this negation makes the
  recovered law obey the project's control contract; it is **not** a claim about
  the original's axis, and it is recorded as unknown #2 in the evidence report.
* **Spawn x fraction −0.6 → −0.35 (retail scene only).** With the original law
  the scripted smoke's steer-into-area builds its heading over ~4 s after the
  reset, so the aircraft reached the hull's `x` range about 350 m of flight
  later — where the hull has tapered away — and a full 60 s run reported
  **zero** obstacle contacts (measured). At −0.35 the same maneuver crosses the
  skin while it is still amidships and the smoke collides again (measured:
  `accept_playtest_retail_launch_scripted_smoke_...` passes). Same knob, same
  kind of measured retune #795 already did for `SPAWN_FRACTION_Z`.
* **Level-Off (Shift+L, command 47) is in the law but unwired**: `cs_types`'s
  `FlightCommand` has no slot for it, so the toggle starts and stays `false`
  rather than being invented. Follow-up **#1134**.
* **The propeller now has two engine sources.** Over original content there is
  no `FlightAircraft` on the body (the F24 record *is* the synthetic law), so
  `spin_propellers` reads `PlaytestOriginalFlight::engine()` — the law's own
  actual (slewed) throttle — beside the F24 record it read before. The retail
  propeller-spin test passes unchanged.
* **`hold_attitude` is a field of the law record**, default `false` (production
  flight), so the acceptance test can fly the held-vertical climb #796's probe
  documents instead of approximating it with stick input.

## Measured, with the acceptance run

* `accept_flight_original_playtest_retail_body_flies_the_imported_parameters` —
  the body carries `PlaytestOriginalFlight` and **no** `FlightAircraft`; its
  `airframe` equals a second, in-process run of the production importer over
  `$CS_GAME_DIR`; the table (engine 0.62, 3.3/7.5/2.0, 3.0, 5.0,
  recI 1.18/1.0/1.1, `fd_speed` 135, 0.37, 1900, 330, `g` 20) matches; mass is
  `W / 9.82` = **193.48268 kg**; the start speed is the declared 55 m/s while
  `fd_speed` stays 135; spawn altitude is below the 2000 m ceiling; provenance
  is `OWNER-STATIC-2026-10-08`; fuel starts at the imported 54 926 and the two
  spawn ticks burn it to 54 925.938. Fails at its first query if `scene.rs`
  reverts to `synthetic_fixed_wing()`.
* `accept_flight_original_playtest_level_full_throttle_reaches_the_cruise_speed`
  — 11 s from level 60 m/s at full throttle settles at **134.3216** m/s
  (134 ±2 %). Starting below the cruise is what makes the assertion mean the
  law's thrust and drag actually ran.
* `accept_flight_original_playtest_vertical_nose_from_500_m_gains_altitude` —
  nose held vertical (the law's own held-attitude probe), full throttle, from
  500 m: **124.673** m gained in 5 s, `nose_y` 1.0000, zero step errors.
* `accept_flight_original_playtest_label_and_docs_name_the_statically_recovered_law`
  — unignored (CI runs it): the label and both playtest documents name
  `OWNER-STATIC-2026-10-08`, "uncalibrated", `#358` and `pbloodhawk`, and the
  label never claims `M01`/`faithful`/`campaign`/`verified_original`.

## Recorded unknowns (not invented)

Carried verbatim into `docs/findings/evidence/T797.json`:

1. `player_spawn_speed` — the original's spawn speed is unknown; the start speed
   is a declared development value. Resolves with #358.
2. `roll_axis_sign` — the original's roll input sign was not recovered; the
   playtest negates to honour the project's control contract.
3. `level_off_command_47` — no input slot; resolves with **#1134**.
4. `scripted_smoke_path` — #649's script was tuned for the synthetic model and
   was not re-derived here (it is outside this task's owner paths); the scene
   knob that keeps it green is measured. Resolves with **#1135**.
5. `law_calibration` — the law itself is still uncalibrated against an original
   run (#358), so nothing here is `verified_original`.

## Tests

Prefix `accept_flight_original_playtest_`, four tests, all green locally with
`CS_GAME_DIR`; the three that read the installation are `#[ignore = "requires
CS_GAME_DIR"]` and run with `--include-ignored`.

Also re-run green after the change, with `CS_GAME_DIR` and the GPU:
`playtest_retail_launch` (10/10, including the scripted retail smoke and the
windowed GPU smoke), `playtest_retail` (37/38: the one failure is
`decal_evidence`, an evidence harness that fails loudly when its `CS_EVIDENCE_*`
inputs are absent — expected outside its own sequence), `playtest_full_aircraft`
(6), `playtest_fly` (8), `playtest_b0004` (1).

`cargo fmt --all -- --check`, `cargo clippy --workspace --all-targets
--all-features --locked -- -D warnings` and `cargo test --workspace --locked`
all pass; CI runs them again on the pushed commit.
