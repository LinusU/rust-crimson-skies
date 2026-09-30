//! A minimal, asset-free fixed-step fixture for the F23-A physics adapter.
//!
//! Spec: `specs/F23-avian-integration-collision-and-fixed-step-authority.md`,
//! stage `### F23-A`. Like [`crate::synthetic`] this is production bootstrap
//! code, not a test-only reimplementation: it builds the real pinned
//! Bevy/Avian plugin group, adds the real [`PhysicsAdapterPlugin`] and spawns
//! one dynamic body with an explicit [`Mass`], so an acceptance test measures
//! the production force path.
//!
//! Gravity is zero and there is exactly one body, so the only velocity change
//! a test observes comes from the force/torque the adapter applied. `SubstepCount(1)`
//! keeps "one integration per tick" literal; the manual clock's baseline is
//! seeded at build time for the reason recorded in
//! `docs/findings/2026-09-23-t334-first-frame-fixed-step.md`.
//!
//! All fixture values are newly authored development content, never original
//! data.

use core::time::Duration;
use std::fmt;

use avian3d::prelude::{
    AngularVelocity, Collider, Gravity, LinearVelocity, Mass, PhysicsPlugins, Position, RigidBody,
    Rotation, SubstepCount,
};
use bevy::{
    prelude::{App, Entity, MinimalPlugins, Transform, TransformPlugin, Vec3, World},
    time::{Real, Time, TimeUpdateStrategy},
};

use super::adapter::{ForceRequest, ForceRequests, PhysicsAdapterPlugin, PhysicsTickLedger};
use super::contacts::PhysicsBodiesPlugin;

/// The body a [`PhysicsFixture`] spawns: an explicit mass and box, at rest.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct FixtureBodySpec {
    /// The body's total mass, in kilograms. Must be strictly positive.
    pub mass_kg: f32,
    /// Half of each box dimension, in meters. Each axis must be strictly
    /// positive.
    pub half_extents_m: [f32; 3],
    /// Initial world position, in meters.
    pub position_m: [f32; 3],
    /// Initial world velocity, in m/s.
    pub linear_velocity_m_s: [f32; 3],
}

impl FixtureBodySpec {
    /// A 1 m cube at the world origin, at rest, with the given mass.
    pub const fn at_origin(mass_kg: f32) -> Self {
        Self {
            mass_kg,
            half_extents_m: [0.5, 0.5, 0.5],
            position_m: [0.0, 0.0, 0.0],
            linear_velocity_m_s: [0.0, 0.0, 0.0],
        }
    }

    /// Validates the fixture before any world is built.
    pub fn validate(&self) -> Result<(), PhysicsFixtureError> {
        for (field, value) in BODY_SCALAR_FIELDS.into_iter().zip([
            self.mass_kg,
            self.position_m[0],
            self.position_m[1],
            self.position_m[2],
            self.linear_velocity_m_s[0],
            self.linear_velocity_m_s[1],
            self.linear_velocity_m_s[2],
        ]) {
            if !value.is_finite() {
                return Err(PhysicsFixtureError::NonFinite { field });
            }
        }
        if self.mass_kg <= 0.0 {
            return Err(PhysicsFixtureError::NonPositiveMass);
        }
        for (axis, extent) in ["x", "y", "z"].into_iter().zip(self.half_extents_m) {
            if !extent.is_finite() {
                return Err(PhysicsFixtureError::NonFinite {
                    field: "half_extents_m",
                });
            }
            if extent <= 0.0 {
                return Err(PhysicsFixtureError::NonPositiveHalfExtent { axis });
            }
        }
        Ok(())
    }
}

const BODY_SCALAR_FIELDS: [&str; 7] = [
    "mass_kg",
    "position_m[0]",
    "position_m[1]",
    "position_m[2]",
    "linear_velocity_m_s[0]",
    "linear_velocity_m_s[1]",
    "linear_velocity_m_s[2]",
];

/// Why a fixture could not be built.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum PhysicsFixtureError {
    /// A named field was NaN or infinite.
    NonFinite { field: &'static str },
    /// `mass_kg` was not strictly positive.
    NonPositiveMass,
    /// A box half extent was not strictly positive.
    NonPositiveHalfExtent { axis: &'static str },
}

impl fmt::Display for PhysicsFixtureError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::NonFinite { field } => write!(f, "{field} must be finite"),
            Self::NonPositiveMass => f.write_str("mass_kg must be greater than zero"),
            Self::NonPositiveHalfExtent { axis } => {
                write!(f, "half_extents_m[{axis}] must be greater than zero")
            }
        }
    }
}

impl std::error::Error for PhysicsFixtureError {}

/// One read-back of the fixture body's pose and velocity.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct PhysicsSample {
    /// Position, in meters.
    pub position_m: [f32; 3],
    /// Linear velocity, in m/s.
    pub linear_velocity_m_s: [f32; 3],
    /// Angular velocity, in rad/s.
    pub angular_velocity_rad_s: [f32; 3],
}

/// One deferred configuration step of a [`PhysicsFixtureBuilder`].
type ConfigureHook = Box<dyn FnOnce(&mut App)>;

/// Builds a [`PhysicsFixture`].
pub struct PhysicsFixtureBuilder {
    spec: FixtureBodySpec,
    fixed_hz: u32,
    configure: Option<ConfigureHook>,
}

impl PhysicsFixtureBuilder {
    /// Overrides the fixed simulation rate. Defaults to
    /// [`BASELINE_FIXED_HZ`](super::BASELINE_FIXED_HZ).
    pub fn fixed_hz(mut self, fixed_hz: u32) -> Self {
        self.fixed_hz = fixed_hz;
        self
    }

    /// Registers extra systems or resources before the world is finalized, the
    /// same seam [`crate::synthetic::SyntheticSceneBuilder::configure`]
    /// exposes.
    pub fn configure(mut self, configure: impl FnOnce(&mut App) + 'static) -> Self {
        self.configure = Some(Box::new(configure));
        self
    }

    /// Builds the fixture, rejecting an invalid body before any world exists.
    pub fn build(self) -> Result<PhysicsFixture, PhysicsFixtureError> {
        let spec = self.spec;
        spec.validate()?;
        assert!(
            self.fixed_hz > 0,
            "PhysicsFixture fixed_hz must be greater than zero"
        );

        let fixed_hz = self.fixed_hz;
        let frame = Duration::from_secs_f64(1.0 / fixed_hz as f64);

        let mut app = App::new();
        app.add_plugins((MinimalPlugins, TransformPlugin, PhysicsPlugins::default()));
        app.add_plugins(PhysicsAdapterPlugin::new(fixed_hz));
        app.add_plugins(PhysicsBodiesPlugin);
        app.insert_resource(TimeUpdateStrategy::ManualDuration(frame));
        app.insert_resource(SubstepCount(1));
        app.insert_resource(Gravity::ZERO);

        // Seed the real clock baseline so the first counted update already
        // produces the full manual delta and one fixed step (see the T334
        // finding referenced in the module doc).
        let startup = app.world().resource::<Time<Real>>().startup();
        app.world_mut()
            .resource_mut::<Time<Real>>()
            .update_with_instant(startup);

        let half = spec.half_extents_m;
        let position = Vec3::from_array(spec.position_m);
        let body = app
            .world_mut()
            .spawn((
                RigidBody::Dynamic,
                Collider::cuboid(half[0] * 2.0, half[1] * 2.0, half[2] * 2.0),
                Mass(spec.mass_kg),
                Transform::from_translation(position),
                Position(position),
                Rotation::default(),
                LinearVelocity(Vec3::from_array(spec.linear_velocity_m_s)),
                AngularVelocity(Vec3::ZERO),
            ))
            .id();

        if let Some(configure) = self.configure {
            configure(&mut app);
        }

        // Finalize the plugin lifecycle before the first manual update; see
        // `SyntheticSceneBuilder::build` for why `App::update` needs this.
        app.finish();
        app.cleanup();

        Ok(PhysicsFixture {
            app,
            body,
            fixed_hz,
            ticks: 0,
        })
    }
}

/// A headless one-body world driven by the real F23-A force adapter.
pub struct PhysicsFixture {
    app: App,
    body: Entity,
    fixed_hz: u32,
    ticks: u64,
}

impl PhysicsFixture {
    /// Starts building a fixture for `spec`.
    pub fn builder(spec: FixtureBodySpec) -> PhysicsFixtureBuilder {
        PhysicsFixtureBuilder {
            spec,
            fixed_hz: super::BASELINE_FIXED_HZ,
            configure: None,
        }
    }

    /// The spawned body's entity.
    pub fn body(&self) -> Entity {
        self.body
    }

    /// The declared fixed rate.
    pub fn fixed_hz(&self) -> u32 {
        self.fixed_hz
    }

    /// The declared fixed timestep, in seconds.
    ///
    /// Derived from the same [`Duration`] the adapter installs, so it matches
    /// Avian's own `Time<Fixed>::timestep().as_secs_f32()` exactly.
    pub fn timestep_s(&self) -> f32 {
        Duration::from_secs_f64(1.0 / self.fixed_hz as f64).as_secs_f32()
    }

    /// Read-only access to the world, for diagnostics and schedule probes.
    pub fn world(&self) -> &World {
        self.app.world()
    }

    /// Mutable access to the world, for tests that must reconfigure a live
    /// body (control-mode transitions, layer rebinding).
    pub fn world_mut(&mut self) -> &mut World {
        self.app.world_mut()
    }

    /// Advances the world by exactly `ticks` fixed steps.
    pub fn step(&mut self, ticks: u64) {
        for _ in 0..ticks {
            self.app.update();
            self.ticks += 1;
        }
    }

    /// Submits one force/torque request for the next fixed tick.
    pub fn submit(&mut self, request: ForceRequest) {
        self.app
            .world_mut()
            .resource_mut::<ForceRequests>()
            .submit(request);
    }

    /// Reads the adapter's tick ledger.
    pub fn ledger(&self) -> PhysicsTickLedger {
        *self.app.world().resource::<PhysicsTickLedger>()
    }

    /// Reads the body's current pose and velocity.
    pub fn sample(&self) -> PhysicsSample {
        let entity = self.app.world().entity(self.body);
        let position = entity
            .get::<Position>()
            .expect("the fixture body always has a Position");
        let velocity = entity
            .get::<LinearVelocity>()
            .expect("the fixture body always has a LinearVelocity");
        let angular = entity
            .get::<AngularVelocity>()
            .expect("the fixture body always has an AngularVelocity");
        PhysicsSample {
            position_m: position.0.to_array(),
            linear_velocity_m_s: velocity.0.to_array(),
            angular_velocity_rad_s: angular.0.to_array(),
        }
    }
}
