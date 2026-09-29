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
//! [`control`] is the F22-A command schema's simulation consumer
//! (`specs/F22-input-bindings-devices-and-control-ownership.md`, stage
//! `### F22-A`): the [`control::ControlBuffer`] that keeps continuous axes
//! and one-shot edges separate and delivers a one-frame key edge exactly once
//! across every physics substep, plus the [`control::ControlGate`] that
//! enforces exactly one control authority and gates local input by
//! `cs_types::input::InputContext`. The device adapters and calibration are
//! F22-B; focus, replay and full ownership wiring are F22-C.
//!
//! [`cs_types`]: cs_types
//! [`cs_script`]: cs_script

pub mod control;
pub mod time;
