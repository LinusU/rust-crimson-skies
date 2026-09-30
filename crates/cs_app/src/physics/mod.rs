//! Avian integration, collision binding and fixed-step authority (F23).
//!
//! Spec: `specs/F23-avian-integration-collision-and-fixed-step-authority.md`.
//! Shared contract: `docs/contracts/FLIGHT-PHYSICS.md`.
//!
//! Stage **F23-A** builds the verified schedule boundary before any runtime:
//! the typed force/torque input, the one-tick request queue, the fixed-rate
//! clock, the tick/integration ledger and a minimal synthetic fixture. Stage
//! **F23-B** adds the runtime that enters the world through it: body creation
//! with the declared collision bindings, the wake path a queued force needs,
//! swept detection for the fast layers, classified contact reports and the
//! control-mode transitions. Render interpolation is F23-C; convergence
//! evidence is F23-D.
//!
//! [`adapter`] pins the schedule F00-B measured: `PhysicsPlugins::default()`
//! integrates in `FixedPostUpdate`, and force requests are drained before
//! `PhysicsSystems::Prepare` so they apply to exactly one tick — a sleeping
//! target is woken before that drain, and a request that reaches no dynamic
//! body is counted as dropped instead of vanishing.
//! [`body`] is the single entry point for creating a runtime body: it
//! validates a [`BodySpec`](body::BodySpec), binds the declared
//! [`CollisionLayer`](cs_sim::collision::CollisionLayer) to Avian's collision
//! groups, turns [`ShapeClass`](cs_sim::collision::ShapeClass) into a sensor,
//! opts the fast layers into swept detection with a bounded speculative
//! margin, and switches control modes without a pose or velocity discontinuity.
//! [`contacts`] classifies Avian's collision events back into that vocabulary
//! and reports each contact episode once.
//! [`fixture`] is the asset-free production harness an acceptance test drives;
//! it spawns a known [`Mass`](avian3d::prelude::Mass) and adds the real
//! adapters.
//!
//! Collision layers are declared in `cs_sim::collision` (the engine-independent
//! vocabulary); this module is the Avian-side execution of those declarations,
//! the forces and the fixed clock.

pub mod adapter;
pub mod body;
pub mod contacts;
pub mod fixture;

pub use adapter::{
    BASELINE_FIXED_HZ, ForceRequest, ForceRequestError, ForceRequests, PhysicsAdapterPlugin,
    PhysicsTickLedger,
};
pub use body::{
    BodyError, BodyLayer, BodyMode, BodySpec, BodyTransitionError, set_body_mode, spawn_body,
};
pub use contacts::{ContactReport, ContactReports, PhysicsBodiesPlugin};
pub use fixture::{
    FixtureBodySpec, PhysicsFixture, PhysicsFixtureBuilder, PhysicsFixtureError, PhysicsSample,
};
