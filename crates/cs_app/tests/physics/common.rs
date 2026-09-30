//! Shared fixtures for the F23-A acceptance tests.
//!
//! Every value is authored here: synthetic masses, forces and geometry. No
//! original game data and no `CS_GAME_DIR` access.

use bevy::prelude::Entity;
use cs_app::physics::{BodySpec, FixtureBodySpec, ForceRequest, PhysicsFixture, spawn_body};

/// A one-body fixture at the world origin with the given mass.
pub(crate) fn fixture(mass_kg: f32) -> PhysicsFixture {
    PhysicsFixture::builder(FixtureBodySpec::at_origin(mass_kg))
        .build()
        .expect("the fixture spec is valid")
}

/// A fixture whose own spare body is parked far from the action, for tests
/// that spawn their bodies through the production path instead.
pub(crate) fn empty_fixture() -> PhysicsFixture {
    PhysicsFixture::builder(FixtureBodySpec {
        mass_kg: 1.0,
        half_extents_m: [0.05, 0.05, 0.05],
        position_m: [-1000.0, 0.0, 0.0],
        linear_velocity_m_s: [0.0, 0.0, 0.0],
    })
    .build()
    .expect("the fixture spec is valid")
}

/// Spawns one body through the production creation path.
pub(crate) fn spawn(fixture: &mut PhysicsFixture, spec: &BodySpec) -> Entity {
    spawn_body(fixture.world_mut(), spec).expect("the spec is valid")
}

/// A pure linear force request (zero torque) for `body`.
pub(crate) fn force_request(body: bevy::prelude::Entity, force_n: [f32; 3]) -> ForceRequest {
    ForceRequest::new(body, force_n, [0.0; 3]).expect("the force is finite")
}

/// A pure torque request (zero force) for `body`.
pub(crate) fn torque_request(body: bevy::prelude::Entity, torque_nm: [f32; 3]) -> ForceRequest {
    ForceRequest::new(body, [0.0; 3], torque_nm).expect("the torque is finite")
}
