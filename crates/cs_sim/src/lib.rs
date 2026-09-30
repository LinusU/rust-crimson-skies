//! Gameplay state, flight forces, combat, AI and objectives.
//!
//! Owner crate per `docs/01-ARCHITECTURE.md`. Allowed dependencies: [`cs_types`]
//! and [`cs_script`]; it consumes normalized records and never parses retail
//! bytes. `bevy_ecs`/`bevy_math` may only enter through an approved boundary,
//! and the physics adapter stays in `cs_app`.
//!
//! [`time`] is the typed-time foundation of
//! `specs/F16-coordinates-units-origin-management-and-clocks.md` (stage
//! F16-A): integer simulation ticks with a fixed dt, the four distinct time
//! domains (simulation, UI wall, unscaled media, authoritative gameplay) and
//! the explicit per-subsystem pause and speed-up policies. Gameplay systems
//! land with the F13+ tasks and consume these clocks instead of inventing
//! their own timers.
//!
//! [`control`] is the F22-A/F22-B command schema's simulation consumer
//! (`specs/F22-input-bindings-devices-and-control-ownership.md`): the
//! [`control::ControlBuffer`] that keeps continuous axes and one-shot edges
//! separate and delivers a one-frame key edge exactly once across every
//! physics substep, the [`control::ControlGate`] that enforces exactly one
//! control authority and gates local input by `cs_types::input::InputContext`,
//! and the F22-B [`control::ThrottleSteps`], whose steps and direct settings
//! are applied at the input boundary so they never depend on the render frame
//! rate. The device adapters and calibration are `cs_app::input::devices`;
//! focus, replay and full ownership wiring are F22-C.
//!
//! [`audio_events`] is the F41-A audio runtime
//! (`specs/F41-audio-music-radio-dialogue-and-spatial-mixing.md`, stage
//! `### F41-A`): the audio-scoped [`audio_events::AudioEventId`] event identity,
//! the bounded [`audio_events::AudioRouter`] whose per-`(session, producer)`
//! sequence ledger accepts a one-shot exactly once and suppresses a replay, and
//! the loop-emitter registry that stops on despawn, swap, declared pause policy
//! or device loss. The provenance-carrying producer record is
//! `cs_content::audio`; the conversion boundary is `cs_app::audio` (F41-B
//! implements decoding, spatial emitters and the real mixer).
//!
//! [`collision`] is the F23-A collision vocabulary
//! (`specs/F23-avian-integration-collision-and-fixed-step-authority.md`,
//! stage `### F23-A`): the six declared [`collision::CollisionLayer`]s, the
//! designed interaction matrix, the [`collision::CollisionLayers`] bitmask and
//! [`collision::classify_contact`], which makes a sensor overlap distinct from
//! a solid contact in code. It creates no Avian body; the schedule adapter is
//! `cs_app::physics` (F23-B/C create and drive the actual bodies).
//!
//! [`damage`] is the F29-A damage contract
//! (`specs/F29-damage-zones-armor-destruction-and-bailout.md`, stage
//! `### F29-A`): the per-actor [`damage::DamageGraph`] of armor zones,
//! internal structure, engines and weapon mounts, the immutable
//! [`damage::HitEvent`] input and ordered [`damage::DamageEvent`] output
//! vocabulary, the five distinct [`damage::LifecycleKind`]s, and the
//! per-session [`damage::DamageResolver`] whose declared hit ordering and
//! per-actor declared attribution award a single kill per destruction.
//! The declared, provenance-carrying schema is `cs_content::damage`; the
//! lowering boundary and ECS bindings are `cs_app::damage`; armor-driven
//! disablement is F29-B and visual/scoring wiring is F29-C.
//!
//! [`animated_object`] is the F20-A animation runtime
//! (`specs/F20-object-animation-and-authored-destruction-states.md`, stage
//! `### F20-A`): the tick-indexed channel records (transform, visibility,
//! material, attachment), the gameplay/presentation event markers with their
//! once-per-activation dedup, the [`animated_object::AnimatedObject`]
//! fixed-tick evaluator whose per-node state keeps mesh and collider on the
//! same evaluated pose, and the minimal synthetic door/propeller fixtures.
//! The declared, provenance-carrying clip form is `cs_content::animation`;
//! the conversion boundary and presentation interpolation are
//! `cs_app::animation` (F20-B wires real tracks, F20-C stateful props).
//!
//! [`flight`] is the F24-A fixed-wing contract and equations
//! (`specs/F24-fixed-wing-flight-engine-stall-and-arcade-assists.md`, stage
//! `### F24-A`): the normalized [`flight::AirframeTuning`] schema, the
//! loadout-mass and damage records, the pure [`flight::FlightModel`] force
//! equations (air-relative velocity, lift/drag, thrust, world-space gravity
//! and a bounded rate-command torque) and the synthetic fixture/probe. At zero
//! airspeed every computed value is finite and gravity still acts. The
//! provenance-carrying tuning schema is `cs_content::flight_tuning`; the
//! Avian wiring, instruments and profile selection are F24-B/F24-C.
//!
//! [`probes`] is the F26-A handling telemetry and reference-envelope schema
//! (`specs/F26-handling-probes-and-original-behavior-calibration.md`, stage
//! `### F26-A`): the closed [`probes::ProbeKind`]/[`probes::ProbeQuantity`]
//! vocabulary, the provenance-carrying [`probes::ReferenceEnvelope`] with its
//! recorded input, initial state, difficulty, loadout, timing uncertainty and
//! units, and [`probes::compare`], the holdout rule that refuses a candidate
//! whose held-out maneuver is outside the envelope. At least one entry must be
//! held out of the fit, and a synthetic envelope can never support an
//! original-fidelity claim. The headless probes that produce a real candidate
//! are F26-B; the roster-wide audit and deviation report is `probes::audit` (F26-C).
//!
//! [`targeting`] is the F30-A targeting contract
//! (`specs/F30-targeting-classification-aim-assistance-and-threat-cues.md`,
//! stage `### F30-A`): the per-session [`targeting::TargetStore`] owning
//! the target roster, the directed [`targeting::AllegianceTable`] and the
//! authoritative-attack threat ledger; the typed [`targeting::TargetRecord`]
//! input, the [`targeting::SelectionRequest`] vocabulary and the read-only
//! [`targeting::TargetInfo`] snapshot HUD and spyglass consume; and the
//! total `(distance, ActorId)` ordering that makes equal-distance cycling
//! deterministic. The declared schema is `cs_content::target_rules`; the
//! conversion boundary and ECS bindings are `cs_app::targeting`; original
//! selection actions are F30-B and verification is F30-D.
//!
//! [`visibility`] is the environment's time domain
//! (`specs/F19-sky-atmosphere-weather-and-visibility.md`, stage `### F19-A`):
//! [`visibility::ENVIRONMENT_TIME_DOMAIN`] puts authored weather changes on
//! authoritative-gameplay time, [`visibility::environment_clock_policy`]
//! states that pause freezes them and that no local authority may inject
//! ticks, and [`visibility::VisibilityTimeline`] installs an event's state
//! only on the whole tick the clock committed — the record it runs is
//! `cs_content::environment::EnvironmentTimeline`, wired in
//! `cs_app::environment`. No value here is derived from screen fog, and no
//! default is invented for a state the caller passes in.
//!
//! [`ai`] is the F31-A navigation contract
//! (`specs/F31-ai-navigation-routes-and-obstacle-avoidance.md`, stage
//! `### F31-A`): the Bevy-free [`ai::navigation::RouteGraph`] with its stable
//! node ids and authored sequences, the [`ai::navigation::ManeuverEnvelope`]
//! that bounds every command, the monotonic [`ai::navigation::RouteProgress`],
//! the swept arrival and [`ai::navigation::Blocker`] tests and
//! [`ai::navigation::Navigator::decide`], a pure function of one typed tick
//! that emits the same [`flight::FlightInput`] a player's controls produce.
//! The provenance-carrying producer record is `cs_content::routes`; F31-B
//! wires pursuit and avoidance into the integrated flight loop.
//!
//! [`environment`] owns the air-relative velocity conversion and nothing else
//! (`specs/F19-sky-atmosphere-weather-and-visibility.md`, non-negotiable
//! behavior 2; task #434 `F19-WIND-CONVERSION-OWNER`):
//! [`environment::air_relative_velocity_m_s`] is the single implementation of
//! the `FLIGHT-PHYSICS` convention `v_air = v_world - wind_world`, and the
//! flight models, `cs_app::environment::air` and every future weapon consumer
//! call it. It sits here because the code that *applies* a wind is below the
//! crate that first needed it, and `cs_app -> cs_sim` is one-way: F19-B could
//! not reach its own conversion from here. No wind record, no still-air default
//! and no wind profile live in this module — reading the authoritative field
//! out of a `cs_content` environment state stays in `cs_app`, because
//! `cs_content` is not a dependency this crate may take.
//!
//! [`cs_types`]: cs_types
//! [`cs_script`]: cs_script

pub mod ai;
pub mod animated_object;
pub mod audio_events;
pub mod collision;
pub mod control;
pub mod damage;
pub mod environment;
pub mod flight;
pub mod probes;
pub mod targeting;
pub mod time;
pub mod visibility;
