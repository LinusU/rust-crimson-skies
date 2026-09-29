//! Shared fixtures for the F23-A acceptance tests.
//!
//! Every value is authored here: synthetic masses, forces and geometry. No
//! original game data and no `CS_GAME_DIR` access.

use cs_app::physics::{FixtureBodySpec, ForceRequest, PhysicsFixture};

/// A one-body fixture at the world origin with the given mass.
pub(crate) fn fixture(mass_kg: f32) -> PhysicsFixture {
    PhysicsFixture::builder(FixtureBodySpec::at_origin(mass_kg))
        .build()
        .expect("the fixture spec is valid")
}

/// A pure linear force request (zero torque) for `body`.
pub(crate) fn force_request(body: bevy::prelude::Entity, force_n: [f32; 3]) -> ForceRequest {
    ForceRequest::new(body, force_n, [0.0; 3]).expect("the force is finite")
}

/// A pure torque request (zero force) for `body`.
pub(crate) fn torque_request(body: bevy::prelude::Entity, torque_nm: [f32; 3]) -> ForceRequest {
    ForceRequest::new(body, [0.0; 3], torque_nm).expect("the torque is finite")
}
