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
//! [`collision`] is the F23-A collision vocabulary
//! (`specs/F23-avian-integration-collision-and-fixed-step-authority.md`,
//! stage `### F23-A`): the six declared [`collision::CollisionLayer`]s, the
//! designed interaction matrix, the [`collision::CollisionLayers`] bitmask and
//! [`collision::classify_contact`], which makes a sensor overlap distinct from
//! a solid contact in code. It creates no Avian body; the schedule adapter is
//! `cs_app::physics` (F23-B/C create and drive the actual bodies).
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
//! [`cs_types`]: cs_types
//! [`cs_script`]: cs_script

pub mod animated_object;
pub mod collision;
pub mod control;
pub mod flight;
pub mod time;
