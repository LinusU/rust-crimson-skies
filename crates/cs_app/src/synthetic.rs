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
//! timestep to the same exactly representable duration, and the manual
//! clock's baseline instant is seeded at build time, so every
//! [`step`](SyntheticScene::step) call advances the world by exactly one
//! simulation tick and one Avian integration step — the first one included
//! (see `docs/findings/2026-09-23-t334-first-frame-fixed-step.md`).

use core::{fmt, time::Duration};

use avian3d::prelude::{Collider, LinearVelocity, PhysicsPlugins, Position, RigidBody};
use bevy::{
    app::App,
    ecs::world::World,
    prelude::{Entity, MinimalPlugins, Resource, Transform, TransformPlugin, Vec3},
    time::{Fixed, Real, Time, TimeUpdateStrategy},
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

/// Configures the scene's [`App`] before plugin finalization.
///
/// This is the seam later stages use to add their own systems to the pinned
/// Bevy/Avian schedules: the closure runs after the synthetic body exists and
/// before [`App::finish`], so systems registered here take part in the very
/// first tick. F00-B uses it to place schedule probes (see
/// `docs/findings/2026-09-23-pinned-bevy-0.19-avian-0.7-schedule-api.md`).
pub struct SyntheticSceneBuilder {
    spec: SyntheticBodySpec,
    configure: Option<ConfigureHook>,
}

/// One deferred configuration step of a [`SyntheticSceneBuilder`].
type ConfigureHook = Box<dyn FnOnce(&mut App)>;

impl SyntheticSceneBuilder {
    /// Registers extra plugins, systems or resources on the scene's world.
    ///
    /// The closure receives the [`App`] that will become the scene; it must
    /// not finalize the plugin lifecycle itself ([`App::finish`] and
    /// [`App::cleanup`] are called by [`SyntheticSceneBuilder::build`]).
    pub fn configure(mut self, configure: impl FnOnce(&mut App) + 'static) -> Self {
        self.configure = Some(Box::new(configure));
        self
    }

    /// Builds the scene, rejecting an invalid spec before any world exists.
    pub fn build(self) -> Result<SyntheticScene, SyntheticSceneError> {
        let spec = self.spec;
        spec.validate().map_err(SyntheticSceneError::InvalidBody)?;

        let mut app = App::new();
        app.add_plugins((MinimalPlugins, TransformPlugin, PhysicsPlugins::default()));

        let frame = Duration::from_secs_f64(1.0 / TICK_HZ);
        app.insert_resource(TimeUpdateStrategy::ManualDuration(frame));
        app.insert_resource(Time::<Fixed>::from_seconds(1.0 / TICK_HZ));
        app.insert_resource(SceneMarkerResource(SceneMarker(SceneProvenance::Synthetic)));

        // The first `update_with_duration` only establishes the real clock's
        // baseline instant and reports a zero delta, so the first update
        // would accumulate nothing and run no fixed step (bevy_time 0.19.1
        // `real.rs`: `update_with_instant` returns early while `last_update`
        // is `None`). Seed that baseline at the startup instant: every
        // counted `App::update` then produces the full manual delta and
        // exactly one fixed step, and all three clocks report
        // `elapsed == ticks * timestep`.
        let startup = app.world().resource::<Time<Real>>().startup();
        app.world_mut()
            .resource_mut::<Time<Real>>()
            .update_with_instant(startup);

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

        if let Some(configure) = self.configure {
            configure(&mut app);
        }

        // Finalize plugin construction before the first manual update.
        //
        // `App::run` would do this for us; driving the world through
        // `App::update` directly means the plugin lifecycle must be closed
        // here, and Bevy documents `cleanup` as the counterpart that exists
        // "for situations where you want to use App::update". A plugin that
        // defers work to `Plugin::cleanup` must not silently never run it.
        app.finish();
        app.cleanup();

        Ok(SyntheticScene {
            app,
            body,
            spec,
            ticks: 0,
        })
    }
}

impl SyntheticScene {
    /// Starts building a scene from a typed spec.
    ///
    /// [`SyntheticScene::new`] is this builder with no extra configuration.
    pub fn builder(spec: SyntheticBodySpec) -> SyntheticSceneBuilder {
        SyntheticSceneBuilder {
            spec,
            configure: None,
        }
    }

    /// Builds the scene, rejecting an invalid spec before any world exists.
    pub fn new(spec: SyntheticBodySpec) -> Result<Self, SyntheticSceneError> {
        Self::builder(spec).build()
    }

    /// Read-only access to the scene's world, for evidence, diagnostics and
    /// schedule probes. Mutation goes through [`Self::step`] so the tick
    /// counter cannot drift from the world state.
    pub fn world(&self) -> &World {
        self.app.world()
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
