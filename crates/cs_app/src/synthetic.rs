//! The asset-free `SYNTHETIC` development scene.
//!
//! This is the production bootstrap path for the pinned Bevy 0.19 / Avian3d
//! 0.7 pair: [`SyntheticScene::new`] validates a typed [`SyntheticBodySpec`],
//! builds a headless world with the real Avian plugin group, spawns exactly
//! one body and marks the world with [`SceneMarker`]. The scene loads no
//! assets, reads no installation and is never selected as a replacement for
//! missing retail content (F00 non-negotiable behavior 2).
//!
//! Determinism: [`TICK_HZ`] fixes both the manual frame delta and the fixed
//! timestep to the same exactly representable duration, so every
//! [`step`](SyntheticScene::step) call advances the world by exactly one
//! simulation tick and one Avian integration step.

use core::{fmt, time::Duration};

use avian3d::prelude::{Collider, LinearVelocity, PhysicsPlugins, Position, RigidBody};
use bevy::{
    app::App,
    prelude::{Entity, MinimalPlugins, Resource, Transform, TransformPlugin, Vec3},
    time::{Fixed, Time, TimeUpdateStrategy},
};
use cs_types::{
    BodyKind, BodySample, SceneMarker, SceneProvenance, SpecError, SyntheticBodySpec, Tick,
};

/// Bevy-side storage for the dependency-free [`SceneMarker`].
///
/// `cs_types` must stay Bevy-free (F00 non-negotiable behavior 1), so the
/// provenance record cannot implement `Resource` itself; this wrapper is the
/// only place the marker enters the ECS world.
#[derive(Resource, Clone, Copy)]
struct SceneMarkerResource(SceneMarker);

/// Fixed simulation rate of the synthetic scene, in ticks per second.
///
/// Chosen as a power of two so `1.0 / TICK_HZ` is exact in binary floating
/// point: the manual frame delta and the fixed timestep are the identical
/// `Duration`, which guarantees one physics step per [`App::update`].
pub const TICK_HZ: f64 = 64.0;

/// Why a synthetic scene could not be built.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum SyntheticSceneError {
    /// The typed input failed validation before any world was built.
    InvalidBody(SpecError),
}

impl fmt::Display for SyntheticSceneError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::InvalidBody(error) => write!(f, "invalid synthetic body spec: {error}"),
        }
    }
}

impl std::error::Error for SyntheticSceneError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::InvalidBody(error) => Some(error),
        }
    }
}

/// A headless, asset-free world containing one synthetic body.
pub struct SyntheticScene {
    app: App,
    body: Entity,
    spec: SyntheticBodySpec,
    ticks: u64,
}

impl SyntheticScene {
    /// Builds the scene, rejecting an invalid spec before any world exists.
    pub fn new(spec: SyntheticBodySpec) -> Result<Self, SyntheticSceneError> {
        spec.validate().map_err(SyntheticSceneError::InvalidBody)?;

        let mut app = App::new();
        app.add_plugins((MinimalPlugins, TransformPlugin, PhysicsPlugins::default()));

        let frame = Duration::from_secs_f64(1.0 / TICK_HZ);
        app.insert_resource(TimeUpdateStrategy::ManualDuration(frame));
        app.insert_resource(Time::<Fixed>::from_seconds(1.0 / TICK_HZ));
        app.insert_resource(SceneMarkerResource(SceneMarker(SceneProvenance::Synthetic)));

        let rigid_body = match spec.kind {
            BodyKind::Dynamic => RigidBody::Dynamic,
            BodyKind::Static => RigidBody::Static,
        };
        let position = Vec3::from_array(spec.position_m);
        let velocity = Vec3::from_array(spec.linear_velocity_m_s);
        let half = spec.half_extents_m;

        let body = app
            .world_mut()
            .spawn((
                rigid_body,
                Collider::cuboid(half[0] * 2.0, half[1] * 2.0, half[2] * 2.0),
                Transform::from_translation(position),
                Position(position),
                LinearVelocity(velocity),
            ))
            .id();

        // Finalize plugin construction before the first manual update.
        //
        // `App::run` would do this for us; driving the world through
        // `App::update` directly means the plugin lifecycle must be closed
        // here, and Bevy documents `cleanup` as the counterpart that exists
        // "for situations where you want to use App::update". A plugin that
        // defers work to `Plugin::cleanup` must not silently never run it.
        app.finish();
        app.cleanup();

        Ok(Self {
            app,
            body,
            spec,
            ticks: 0,
        })
    }

    /// Advances the world by `ticks` simulation ticks, exactly one Avian
    /// integration step each ([`TICK_HZ`]).
    pub fn step(&mut self, ticks: u64) {
        for _ in 0..ticks {
            self.app.update();
            self.ticks += 1;
        }
    }

    /// Reads the typed output sample for the synthetic body.
    pub fn sample(&self) -> BodySample {
        let entity = self.app.world().entity(self.body);
        let position = entity
            .get::<Position>()
            .expect("the synthetic body always has a Position");
        let velocity = entity
            .get::<LinearVelocity>()
            .expect("the synthetic body always has a LinearVelocity");
        BodySample {
            tick: Tick(self.ticks),
            position_m: position.0.to_array(),
            linear_velocity_m_s: velocity.0.to_array(),
        }
    }

    /// Reads the provenance marker out of the world itself, so a scene that
    /// lost its `SYNTHETIC` marking fails loudly instead of passing silently.
    pub fn provenance(&self) -> SceneProvenance {
        self.app.world().resource::<SceneMarkerResource>().0.0
    }

    /// The typed input this scene was built from.
    pub fn spec(&self) -> &SyntheticBodySpec {
        &self.spec
    }
}
