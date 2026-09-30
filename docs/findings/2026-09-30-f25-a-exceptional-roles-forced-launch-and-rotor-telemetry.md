# F25-A: exceptional model roles, forced launch assignment and rotor telemetry

Date: 2026-09-30. Task: F25-A "Define exceptional model roles and autogyro
telemetry" (`specs/F25-hoplite-autogyro-and-exceptional-flight-configurations.md`,
section `### F25-A`). Shared contract: `docs/contracts/FLIGHT-PHYSICS.md`.
Capabilities used: ordinary build/test only. No `CS_GAME_DIR` read, no
`retail`/`gpu`/`audio`, therefore **no evidence report** is required or
produced: this stage ships types, a synthetic fixture and a pure resolver, and
awards at most **checked** — never `verified_original`.

## Files and the one observable failure (listed before editing)

- `crates/cs_sim/src/flight/autogyro.rs` (new, 1494 lines): the typed
  exceptional boundary — `FlightTelemetry` (the shared HUD/AI/probe interface),
  `SharedTelemetry`, `TelemetryFrame`, `RotorDrive`, `RotorSpeedMapping`,
  `RotorTelemetry`, `RotorVisualSample`, `ManeuverKind`, `ManeuverSpec`,
  `EnvelopeStatus`, `ReferenceManeuverEnvelope`, and the synthetic fixture
  (`synthetic_rotor_mapping`, `synthetic_rotor_drive`,
  `synthetic_exceptional_envelope`, `SYNTHETIC_TICK_DT_S`).
- `crates/cs_content/src/airframe_roles.rs` (new, 977 lines): the
  provenance-bearing role record (`AirframeRole`, `Availability`,
  `LaunchConstraints`, `WeaponConstraints`, `RotorRole`, `AirframeRoles`) and
  AC01's resolver (`OwnedLoadout`, `ForcedAssignment`, `LaunchSource`,
  `ResolvedLaunch`, `LaunchAssignmentError`,
  `AirframeRoles::resolve_launch`), plus `declared_synthetic_roles`.
- `crates/cs_app/src/airframe_visual.rs`: `RotorVisualBinding`,
  `AirframeVisual::bind_rotor`/`rotors`/`rotor`, and the three new error
  variants. The pre-existing `accept_f11_a_*` test is untouched.
- `crates/cs_sim/src/flight/mod.rs`, `crates/cs_content/src/lib.rs` (wiring
  only): `pub mod autogyro;`, `pub mod airframe_roles;`, the re-exports and the
  doc paragraphs.
- `crates/cs_sim/tests/accept_f25_a_exceptional_telemetry.rs`,
  `crates/cs_content/tests/accept_f25_a_forced_launch.rs`,
  `crates/cs_app/tests/accept_f25_a_rotor_visual.rs` (new): the integration
  acceptance tests.
- This file.

**One observable failure:** with AC01's forced-assignment branch removed from
`AirframeRoles::resolve_launch`, a mission that hands the session a
mission-only autogyro while the player's hangar selection names the shop-listed
fixed wing spawns *the fixed wing instead* — the requested actor never appears,
and nothing reports a refusal.
`accept_f25_a_forced_launch_uses_the_requested_actor_not_the_garage_plane`
fails on exactly that, on `resolved.airframe == assignment.airframe` and on
`resolved.airframe != garage.airframe`.

## What the slice does, per sheet rule

**AC01 — forced launch.** `resolve_launch(&OwnedLoadout, Option<&ForcedAssignment>)`
is a pure function over the roster. With a forced assignment it returns the
assignment's airframe, `LaunchSource::ForcedMissionAssignment` and the session's
generation; `OwnedLoadout` is taken **by shared reference**, so a mission cannot
write to what the player owns, and `ResolvedLaunch::persists_to_owned_loadout()`
is `false` for a forced launch so a save made mid-mission cannot adopt it. The
five refusals (`SessionGenerationMismatch`, `UnknownAirframe`, `NotPilotable`,
`NotMissionLaunchable`, `NotHangarSelectable`) never degrade into a silent
fallback to the hangar plane.

**Non-negotiable 2 (session scope).** The `SessionGenerationMismatch` refusal
is what implements "only for that session": M17's own sheet lists "the wrong
actor, wrong session or repeated event" as the regression priority, so an
assignment carrying another generation is refused instead of applied.

**Non-negotiable 1 (no invented helicopter).** This stage adds **no force law,
no lift curve and no hover**. The role records *capability*, not dynamics, and
the maneuver envelope ships `EnvelopeStatus::Unmeasured` with the reason
recorded, so
`ReferenceManeuverEnvelope::is_ready_as_reference()` is `false`. The
`ModelKind` vocabulary is `fixed_wing`/`exceptional`; the test asserts that
`"helicopter"` and `"gyrocopter"` are refused as unknown kinds, so the word
cannot become a kind.

**Non-negotiable 3 (physical vs. visual rotor).** Two mechanisms, not a
comment:

1. *The mapping must be explicit.* `RotorSpeedMapping::new` refuses a
   non-positive or non-finite ratio, and every rotor accessor takes
   `Option<&RotorSpeedMapping>`; with `None` the channel reports **no** visual
   rate at all instead of an implicit `1.0`.
2. *Animation cannot drive physics.* `RotorDrive::advance_tick` is the only
   `&mut self` mutator of `physical_speed_radps` and it refuses a tick that is
   not strictly newer than `last_tick` (`TelemetryError::NonMonotonicTick`).
   `RotorVisualBinding::sample` and `RotorDrive::visual_sample` take `&self`, so
   the render frame time cannot reach the simulation even in principle. The
   tests draw one frame per tick and sixty frames per tick and assert the
   authoritative rate and the drawn phase are identical.

**Non-negotiable 4 (mission-only models).** `Availability::MissionOnly` is
roster presence with no shop listing. The validation refuses a mission-only
role that also declares itself hangar-selectable, and the resolver rejects a
mission-only airframe as a *hangar* selection while accepting it through a
forced assignment.

**Non-negotiable 5 (shared telemetry).** `FlightTelemetry` is the one interface;
`SharedTelemetry::sample` copies numbers a production `FlightOutput` already
produced, so a consumer holding `&dyn FlightTelemetry` reads identical shared
values from both kinds — proven by taking both frames as trait objects in
`accept_f25_a_telemetry_consumer_is_model_agnostic_across_both_kinds`. The
exceptional channel is an optional accessor, not a downcast. The declared kind
and the present channels must agree
(`TelemetryError::ModelKindMismatch`).

**The contract's "separate reference maneuver envelope".**
`ReferenceManeuverEnvelope` is a closed vocabulary: the eight maneuvers
`FLIGHT-PHYSICS` "Calibration acceptance" names are required for every airframe,
and `ManeuverKind::required_for(ModelKind::Exceptional)` additionally requires
the four the F25 sheet names for an autogyro — low speed, yaw, lift and rotor
visual. A missing required maneuver is refused by name. Each `ManeuverSpec`
carries initial conditions, input timing, measurement error and a tolerance
selected before the fit, and at least one maneuver must be `held_out`.

## What is deliberately *not* here

- **No control law, no telemetry consumer wiring.** F25-B implements the
  measured exceptional control law; F25-C wires the role record, the resolver
  and the telemetry into the real producers and consumers (mission load, HUD,
  AI, damage, unload). This stage is types plus one pure function, as the
  sheet's `### F25-A` requires.
- **No roster read.** `declared_synthetic_roles()` is the only declared roster
  and it is `Origin::SyntheticFixture`. Every numeric role field is an explicit
  `Resolved::unknown` with a reason — including the exceptional rotor ratio,
  because no original measurement of a visual/physical rotor ratio exists. A
  consumer built from that record reports **no** visual rotor rate, which is the
  honest behavior.

## Unknowns recorded (not guessed)

| Unknown | Affected content | How this stage represents it |
| --- | --- | --- |
| The autogyro's exact control law | M17 "The Pirate's Duel" and any other forced-autogyro mission | no force law exists; the envelope ships `Unmeasured` and is not reference-ready |
| A visual/physical rotor speed ratio for any real airframe | every rotor-bearing airframe's visual | `RotorRole::visual_radps_per_physical_radps` is `Resolved::unknown`; the numeric channel answers `None` |
| The original roster: which airframes exist, and which are shop-listed vs. mission-only | the whole exceptional role catalog | only the two synthetic roles are declared; no original roster was read |
| Which catalog id, script or mission switch expresses "forced autogyro" | M17's forced-airframe boundary | `docs/research/mission-discovery.json` carries `"catalog_id": null, "status": "unverified"`; `missions/M17.md` says the cue is a research label. The resolver takes an explicit `ForcedAssignment` and does not key anything on a mission id or a filename |
| The launch airspeed and hardpoint counts for the fixture roles | the synthetic roster only | `Resolved::unknown` with a reason |
| Whether an exceptional airframe may carry weapons in the original | exceptional weapon constraints | `WeaponConstraints::armed` is `false` for the synthetic autogyro role and its hardpoint count is unknown; no original value is claimed |

`Hoplite` is a source-observed name/prefix only; the original localized string,
the catalog id and the bindings are unverified (`F25` "Research boundary",
`missions/M17.md`, `docs/research/mission-discovery.json`).

## Test sensitivity (verified by perturbation, not asserted)

Each behavior was removed temporarily and the named test re-run:

| Behavior removed | Tests that failed |
| --- | --- |
| the forced branch of `resolve_launch` (falls back to the hangar plane) | `accept_f25_a_forced_launch_uses_the_requested_actor_not_the_garage_plane` (unit **and** integration) |
| the strictly-newer-tick check in `RotorDrive::advance_tick` | `accept_f25_a_rotor_rate_is_integrated_only_by_the_fixed_tick`, `accept_f25_a_rotor_rate_advances_only_on_a_newer_fixed_tick` |
| the explicit-mapping requirement in `RotorDrive::telemetry` (implicit 1:1 when `mapping == None`) | `accept_f25_a_visual_rotor_rate_requires_the_explicit_mapping`, `accept_f25_a_visual_rotor_rate_needs_a_declared_mapping` |

All three perturbations were reverted and the suite re-run green.

The reviewer added four more probes, one per review correction, each applied,
run and reverted from a byte-identical backup:

| Behavior removed | Tests that failed |
| --- | --- |
| the produced-sample check in `RotorDrive::visual_sample` | `accept_f25_a_visual_rotor_sample_never_returns_a_nonfinite_phase`, `accept_f25_a_rotor_visual_sampling_never_reaches_the_simulation` |
| the root-descendant check in `AirframeVisual::bind_rotor` | `accept_f25_a_rotor_visual_binds_by_id_and_cannot_drive_physics` |
| `origin.is_original()` in `is_ready_as_reference` | `accept_f25_a_exceptional_envelope_demands_its_maneuvers_and_is_unmeasured` |
| the `MeasuredWithoutObservation` refusal in `validate` | `accept_f25_a_exceptional_envelope_demands_its_maneuvers_and_is_unmeasured` |

## Limits of this pass

- The declared roster is synthetic. Nothing here has been checked against the
  owner's installation, and no field may be read as an original value.
- `resolve_launch` is a pure resolver with no producer yet. M17's forced
  assignment is not bound to a script, a catalog id or a mission program; that
  binding is F25-C/F25-D and needs the original data.
- `resolve_launch` cannot tell a free-flight session from a mission session that
  simply has no forced assignment, so it checks `hangar_selectable` and
  `pilotable` on the garage path and leaves `mission_launchable` to the forced
  path (see the field's doc comment). A third rule for "may this mission use the
  garage plane at all" is **not** invented here; F25-C's wiring must decide it
  from the mission data rather than from this record.
- `TelemetryFrame` is produced from a **fixed-wing** `FlightOutput` in the tests
  because F25-B's exceptional law does not exist yet. Once it does, the shared
  channel is unchanged by construction, but the exceptional frame's shared
  numbers have not been observed against a real exceptional law.
- No consumer (HUD, AI, probe) reads `FlightTelemetry` yet; the interface is
  proven model-agnostic by construction and by a trait-object test, not by a
  runtime consumer trace.
- A rotor node is bound by content id **under the airframe's own root**
  (`<container>.<root>.<part>`), which assumes the authored node paths put an
  airframe's parts under that airframe's root. F11's node keys
  (`planes.corsair.wing_l`) are consistent with it, but no original node array
  has been read by this stage: if a real rotor ever appears as a sibling root,
  F25-C/D must relax the rule with the evidence, not by guessing now.
- F25-D (retail) still needs an actual integration/reference evidence run, and
  AC04's "compare the distinctive handling against the original" is entirely
  out of this stage's reach.

## Review corrections (2026-09-30, `bunny-alpha-2` reviewing #97)

Four problems found in review, fixed on the task branch rather than handed
back. Each is a boundary that accepted something it should have refused, or a
documented rule the code did not match.

1. **The drawn rotor phase could leave the boundary as `NaN`.**
   `RotorDrive::visual_sample` validated its inputs but not the sample it
   produced: a finite physical rate of `1e300` times a declared ratio of `1e300`
   and a finite frame time overflowed to `inf`, and `inf.rem_euclid(TAU)` is
   `NaN`, which was returned as `Ok`. The sample is now checked before it is
   handed out (`RotorVisualSample::validate`), so an overflowing draw is refused
   by name. New test `accept_f25_a_visual_rotor_sample_never_returns_a_nonfinite_phase`
   plus an assertion in the integration telemetry test.

2. **A rotor could bind another airframe's node.** `bind_rotor` checked only
   that the node lived in the airframe's *container*, so `planes.corsair_mk2.rotor_main`
   — a sibling root's part in the same container — was accepted and would have
   spun this airframe's rotor on somebody else's node. The check now requires the
   node to descend from this airframe's root (`RotorNodeOutsideAirframe`), the
   root node itself is refused (spinning the whole airframe is not a rotor), and
   the container refusal is kept for a node from another tree. Covered in the
   `cs_app` unit test and in `accept_f25_a_rotor_visual_binds_a_node_under_the_airframe_root`.

3. **A fixture could declare itself an approved reference trace.**
   `is_ready_as_reference()` only asked for a `Measured` status and full
   coverage, so setting that status on the synthetic fixture made it
   reference-ready — with a *designed* provenance, which is not a measurement
   at all. Readiness now also requires an `Origin::Installation` envelope, and
   `validate()` refuses a `Measured` status whose provenance class is not
   `ObservedTool` or `VerifiedOriginal` (`EnvelopeError::MeasuredWithoutObservation`).
   The tests now assert the honest negative: a designed provenance is refused,
   an observed provenance on a synthetic envelope is still not ready, and only
   an installation-sourced envelope with an observed reference, full coverage
   and a held-out maneuver is. The synthetic `SourceSpan` those tests construct
   names **no** real installation and backs no original-data claim.

4. **`mission_launchable` was documented as a rule the resolver did not apply.**
   The field said "whether a mission may launch this airframe at all" while the
   hangar path checked only `pilotable` and `hangar_selectable`. The field and
   `resolve_launch`'s docs now state exactly what each path checks and why
   (`mission_launchable` gates *assignments*; the garage plane is the player's
   own plane), and the limit is recorded above for F25-C. No behavior was
   changed: refusing a hangar-only plane on the garage path would make the
   role meaningless, and adding a launch-context parameter would guess at a
   mission rule this stage has no data for.

### Reviewer notes

Implementer: `bunny-alpha-2` (Space Bunny Alpha) on branch
`rally/97-define-exceptional-model-roles-and-autog`. Reviewer: `bunny-alpha-2`
(Space Bunny Alpha) again — the **same agent instance and model** as the
implementer, in a fresh session that rebuilt its context from the spec, the
contract and the diff. Per the owner directive a different agent instance or
model is preferred for fidelity claims; this review is therefore independent by
process only, not by identity, and it makes no original-reference claim.

The reviewer re-ran all four required checks locally, reproduced the
implementer's three mutation probes, and added four probes of its own for the
corrections above (see the sensitivity table). No protected path, no original
data and no binary file is touched: `git diff --name-only origin/main...HEAD`
lists the three owner source files, their two wiring files, the three test files
and this note.

Still **checked**, never `verified_original` or `release_approved`: this stage
ships a typed boundary, a synthetic fixture and one pure resolver, and
`crates/cs_app/src/physics/flight.rs` still refuses `ModelKind::Exceptional`
(#414), so no exceptional airframe can be flown yet.