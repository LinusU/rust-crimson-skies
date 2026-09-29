//! Avian integration, collision binding and fixed-step authority (F23).
//!
//! Spec: `specs/F23-avian-integration-collision-and-fixed-step-authority.md`.
//! Shared contract: `docs/contracts/FLIGHT-PHYSICS.md`.
//!
//! Stage **F23-A** builds the verified schedule boundary before any runtime:
//! the typed force/torque input, the one-tick request queue, the fixed-rate
//! clock, the tick/integration ledger and a minimal synthetic fixture. The
//! body-creation, sweep and kinematic-transition runtime is F23-B; render
//! interpolation is F23-C; convergence evidence is F23-D.
//!
//! [`adapter`] pins the schedule F00-B measured: `PhysicsPlugins::default()`
//! integrates in `FixedPostUpdate`, and force requests are drained before
//! `PhysicsSystems::Prepare` so they apply to exactly one tick.
//! [`fixture`] is the asset-free production harness an acceptance test drives;
//! it spawns a known [`Mass`](avian3d::prelude::Mass) and adds the real adapter.
//!
//! Collision layers are declared in `cs_sim::collision` (the engine-independent
//! vocabulary); this module is only the Avian-side execution of forces and the
//! fixed clock.

pub mod adapter;
pub mod fixture;

pub use adapter::{
    BASELINE_FIXED_HZ, ForceRequest, ForceRequestError, ForceRequests, PhysicsAdapterPlugin,
    PhysicsTickLedger,
};
pub use fixture::{
    FixtureBodySpec, PhysicsFixture, PhysicsFixtureBuilder, PhysicsFixtureError, PhysicsSample,
};
