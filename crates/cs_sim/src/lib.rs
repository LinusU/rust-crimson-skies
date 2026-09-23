//! Gameplay state, flight forces, combat, AI and objectives.
//!
//! Owner crate per `docs/01-ARCHITECTURE.md`. No implementation yet: gameplay
//! systems land with the F13+ tasks. Allowed dependencies: [`cs_types`] and
//! [`cs_script`]; it consumes normalized records and never parses retail
//! bytes. `bevy_ecs`/`bevy_math` may only enter through an approved boundary,
//! and the physics adapter stays in `cs_app`.
//!
//! [`cs_types`]: cs_types
//! [`cs_script`]: cs_script
