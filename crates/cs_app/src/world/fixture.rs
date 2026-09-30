//! The minimal synthetic world fixture and the swept probe (F18-A).
//!
//! Spec: `specs/F18-world-geometry-terrain-water-and-traversable-interiors.md`,
//! stage `### F18-A`.
//!
//! Like [`crate::synthetic`] and [`crate::physics::fixture`], this is
//! **production bootstrap code**, not a test-only reimplementation: it authors
//! a [`WorldDefinition`] through the same records a real importer will
//! produce, and [`spawn_swept_probe`] spawns a body through the same Avian
//! components the runtime uses. The acceptance tests drive these functions;
//! they do not carry their own world builder.
//!
//! # The arch
//!
//! ```text
//!        z
//!        ^      +------------------+
//!        |      |      lintel      |        y
//!        |      +----+      +------+        ^
//!        |      |    |      |    |          |
//!   -----+------+------+----+----+-----> x  |
//!        |  L |      open- |  R |           |
//!        |    |     ing    |    |           |
//!   -----+----+------------+----+-----      |
//!              |    ground slab    |
//! ```
//!
//! The opening spans `z ∈ (-1, 1)` and `y ∈ (0, 3)`; the legs and the lintel
//! are 1 m thick along the flight axis `x`. The opening is defined by the
//! *same* box records the colliders are built from, so "the hole in the
//! render" and "the hole in the collision" cannot be authored separately: a
//! body that fits through the visual gap also fits through the collision gap.
//!
//! Everything here is newly authored synthetic fixture content
//! (`Origin::SyntheticFixture`); it never claims to be original geometry.
//! Which geometry the original worlds contain, and how they store sectors, is
//! unmeasured — see
//! `docs/findings/2026-09-30-f18-a-world-instances-sectors-and-collision-roles.md`.

use std::collections::BTreeSet;
use std::time::Duration;

use avian3d::prelude::{
    AngularVelocity, Collider, CollisionEventsEnabled, CollisionLayers as AvianCollisionLayers,
    Gravity, LinearVelocity, Mass, PhysicsPlugins, Position, RigidBody, Rotation,
    SpeculativeMargin, SubstepCount, SweptCcd,
};
use bevy::prelude::{App, Entity, MinimalPlugins, Transform, TransformPlugin, Vec3};
use bevy::time::{Real, Time, TimeUpdateStrategy};
use cs_content::scene::CanonicalTransform;
use cs_content::world::{
    Aabb, Sector, SectorId, SurfaceRole, WorldBoundary, WorldCollisionRole, WorldCollisionShape,
    WorldDefinition, WorldError, WorldId, WorldObjectId, WorldObjectInstance,
};
use cs_types::content::{ContentId, ContentKind, Known, Origin, Provenance, Resolved};
use cs_types::evidence::ClaimId;

use super::spawn::{SpawnedWorld, avian_layers};
use cs_sim::collision::{CollisionLayer, CollisionLayers};

// ------------------------------------------------------------- arch metrics ---

/// Half extents of one arch leg: 1 m thick along the flight axis, 3 m tall,
/// 1 m deep.
pub const ARCH_LEG_HALF_M: [f64; 3] = [0.5, 1.5, 0.5];

/// How far each leg's centre sits from the opening's centreline in `z`.
///
/// The opening therefore spans `z ∈ (-1, 1)`: `ARCH_LEG_Z_M - ARCH_LEG_HALF_M[2]`.
pub const ARCH_LEG_Z_M: f64 = 1.5;

/// Half extents of the lintel that closes the arch above the opening.
pub const ARCH_LINTEL_HALF_M: [f64; 3] = [0.5, 0.5, 2.0];

/// The lintel's centre: its underside is exactly [`ARCH_OPENING_TOP_M`].
pub const ARCH_LINTEL_POS_M: [f64; 3] = [0.0, 3.5, 0.0];

/// The opening's half-width in `z`.
pub const ARCH_OPENING_HALF_Z_M: f64 = 1.0;

/// The opening's height: the ground surface is `y = 0`, the lintel's
/// underside is `y = 3`.
pub const ARCH_OPENING_TOP_M: f64 = 3.0;

/// The arch's half-thickness along the flight axis `x`.
pub const ARCH_HALF_X_M: f64 = 0.5;

/// Half extents of the ground slab under the arch.
pub const GROUND_HALF_M: [f64; 3] = [20.0, 0.5, 15.0];

/// The ground slab's centre: its top face is `y = 0`.
pub const GROUND_POS_M: [f64; 3] = [0.0, -0.5, 0.0];

/// Half extents of the rotated water patch, deliberately bounded: water is a
/// *role on a patch*, never an infinite collision plane (F18 non-negotiable
/// behavior 2).
pub const WATER_HALF_M: [f64; 3] = [4.0, 0.1, 4.0];

/// The water patch's centre, away from the flight path.
pub const WATER_POS_M: [f64; 3] = [0.0, 0.05, -16.0];

/// The water patch's rotation about `+y`, in radians. A rotated instance
/// keeps the transform check honest: a translation-only fixture could not
/// tell a rotation bug from correct placement.
pub const WATER_ROTATION_RAD: f64 = std::f64::consts::FRAC_PI_6;

// ---------------------------------------------------------------- identity ---

/// The synthetic world's id key.
pub const WORLD_KEY: &str = "synthetic.arch_world";

/// Sector on the `x < -1` side of the arch.
pub const SECTOR_APPROACH: &str = "approach";
/// Sector containing the arch itself, `x ∈ [-1, 1]`.
pub const SECTOR_ARCH: &str = "arch";
/// Sector on the `x > 1` side of the arch.
pub const SECTOR_BEYOND: &str = "beyond";

/// The left arch leg.
pub const OBJECT_LEG_LEFT: &str = "arch.leg_left";
/// The right arch leg — the one the failure-case probe is aimed at.
pub const OBJECT_LEG_RIGHT: &str = "arch.leg_right";
/// The lintel above the opening.
pub const OBJECT_LINTEL: &str = "arch.lintel";
/// The ground slab; it belongs to every sector, so it never streams away
/// while any part of the world is resident.
pub const OBJECT_GROUND: &str = "terrain.ground";
/// The rotated water patch; it belongs to no sector, so it is resident.
pub const OBJECT_WATER: &str = "water.patch";
/// An object whose collision role the evidence never resolved.
pub const OBJECT_UNEVIDENCED_ROLE: &str = "sign.unevidenced_role";
/// An object with a solid role but an unresolved collision shape.
pub const OBJECT_UNEVIDENCED_SHAPE: &str = "hangar.unevidenced_shape";

// --------------------------------------------------------------- provenance ---

fn claim(key: &str) -> ClaimId {
    ClaimId::new(&format!("f18a.{key}")).expect("the fixture claim ids are valid")
}

/// Designed provenance for a fixture claim.
#[must_use]
pub fn fixture_provenance(key: &str) -> Provenance {
    Provenance::designed(claim(key))
}

fn known<T>(value: T, key: &str) -> Resolved<T> {
    Resolved::Known(Known::new(value, fixture_provenance(key)))
}

fn unknown<T>(key: &str, reason: &str) -> Resolved<T> {
    Resolved::unknown(claim(key), reason).expect("the fixture reasons are non-empty")
}

/// The identity transform at `translation`, in meters.
fn translated(translation: [f64; 3]) -> CanonicalTransform {
    CanonicalTransform::try_new(
        [[1.0, 0.0, 0.0], [0.0, 1.0, 0.0], [0.0, 0.0, 1.0]],
        translation,
    )
    .expect("the fixture translations are finite")
}

/// A rotation about `+y` by `angle` radians, then a translation.
fn rotated_y(translation: [f64; 3], angle: f64) -> CanonicalTransform {
    let (sin, cos) = angle.sin_cos();
    CanonicalTransform::try_new(
        [[cos, 0.0, sin], [0.0, 1.0, 0.0], [-sin, 0.0, cos]],
        translation,
    )
    .expect("the fixture rotation is finite")
}

fn mesh(key: &str) -> Resolved<ContentId> {
    known(
        ContentId::from_source(ContentKind::Mesh, &format!("synthetic.{key}"))
            .expect("the fixture mesh ids are valid"),
        &format!("{key}.mesh"),
    )
}

fn sector(key: &str) -> SectorId {
    SectorId::new(key).expect("the fixture sector keys are valid")
}

fn object(key: &str) -> WorldObjectId {
    WorldObjectId::new(key).expect("the fixture object keys are valid")
}

fn cuboid(half: [f64; 3]) -> Resolved<WorldCollisionShape> {
    known(
        WorldCollisionShape::cuboid(half).expect("the fixture half extents are positive"),
        "fixture.cuboid",
    )
}

/// A solid box instance in `sectors`.
fn solid_box(
    key: &str,
    transform: CanonicalTransform,
    half: [f64; 3],
    surface: Resolved<SurfaceRole>,
    sectors: Vec<SectorId>,
) -> WorldObjectInstance {
    WorldObjectInstance::try_new(
        object(key),
        mesh(key),
        transform,
        known(WorldCollisionRole::Solid, "fixture.collision-role.solid"),
        cuboid(half),
        surface,
        sectors,
        fixture_provenance(&format!("{key}.record")),
    )
    .expect("the fixture sector lists contain no duplicates")
}

// ------------------------------------------------------------------ fixture ---

/// Builds the synthetic arch world.
///
/// Seven object instances across three sectors, plus one resident water
/// patch: three solid arch parts that define the traversable opening, a
/// ground slab spanning every sector, a rotated water patch that belongs to
/// no sector, and two objects that carry an **explicit unknown** so the
/// unresolved paths are exercised by the same fixture the happy paths are.
///
/// # Errors
///
/// Never for this fixture — its ids, transforms and shapes are constants
/// validated on the path here — but kept as `Result` so a later fixture that
/// takes parameters has the same shape as the importer's own constructor.
pub fn arch_world() -> Result<WorldDefinition, WorldError> {
    let approach = sector(SECTOR_APPROACH);
    let arch = sector(SECTOR_ARCH);
    let beyond = sector(SECTOR_BEYOND);

    let sectors = vec![
        Sector::new(
            approach.clone(),
            Aabb::try_new([-40.0, -2.0, -6.0], [-1.0, 10.0, 6.0])
                .expect("the approach bounds are well formed"),
        ),
        Sector::new(
            arch.clone(),
            Aabb::try_new([-1.0, -2.0, -6.0], [1.0, 10.0, 6.0])
                .expect("the arch bounds are well formed"),
        ),
        Sector::new(
            beyond.clone(),
            Aabb::try_new([1.0, -2.0, -6.0], [40.0, 10.0, 6.0])
                .expect("the beyond bounds are well formed"),
        ),
    ];

    let ground_surface = known(SurfaceRole::Ground, "fixture.surface.ground");
    let water_surface = known(SurfaceRole::Water, "fixture.surface.water");
    let no_surface = unknown::<SurfaceRole>(
        "surface.unmeasured",
        "no evidence has classified this instance's gameplay surface",
    );

    let objects = vec![
        solid_box(
            OBJECT_LEG_LEFT,
            translated([0.0, ARCH_LEG_HALF_M[1], -ARCH_LEG_Z_M]),
            ARCH_LEG_HALF_M,
            ground_surface.clone(),
            vec![arch.clone()],
        ),
        solid_box(
            OBJECT_LEG_RIGHT,
            translated([0.0, ARCH_LEG_HALF_M[1], ARCH_LEG_Z_M]),
            ARCH_LEG_HALF_M,
            ground_surface.clone(),
            vec![arch.clone()],
        ),
        solid_box(
            OBJECT_LINTEL,
            translated(ARCH_LINTEL_POS_M),
            ARCH_LINTEL_HALF_M,
            ground_surface.clone(),
            vec![arch],
        ),
        solid_box(
            OBJECT_GROUND,
            translated(GROUND_POS_M),
            GROUND_HALF_M,
            ground_surface.clone(),
            vec![approach.clone(), beyond.clone()],
        ),
        solid_box(
            OBJECT_WATER,
            rotated_y(WATER_POS_M, WATER_ROTATION_RAD),
            WATER_HALF_M,
            water_surface,
            vec![],
        ),
        // The collision role itself is unevidenced: the record carries the
        // unknown, and the spawn reports it instead of choosing a side.
        WorldObjectInstance::try_new(
            object(OBJECT_UNEVIDENCED_ROLE),
            mesh(OBJECT_UNEVIDENCED_ROLE),
            translated([-10.0, 2.0, 4.0]),
            unknown::<WorldCollisionRole>(
                "collision-role.unmeasured",
                "no evidence has classified whether this instance blocks motion",
            ),
            cuboid([0.5, 0.5, 0.1]),
            no_surface,
            vec![approach],
            fixture_provenance("sign.unevidenced_role.record"),
        )
        .expect("the fixture sector lists contain no duplicates"),
        // A solid role whose *shape* is unevidenced: the spawn must report
        // the missing geometry rather than substitute a box for it.
        WorldObjectInstance::try_new(
            object(OBJECT_UNEVIDENCED_SHAPE),
            mesh(OBJECT_UNEVIDENCED_SHAPE),
            translated([20.0, 3.0, -5.0]),
            known(WorldCollisionRole::Solid, "fixture.collision-role.solid"),
            unknown::<WorldCollisionShape>(
                "collision-shape.unmeasured",
                "no collider geometry has been measured for this instance",
            ),
            ground_surface.clone(),
            vec![beyond],
            fixture_provenance("hangar.unevidenced_shape.record"),
        )
        .expect("the fixture sector lists contain no duplicates"),
    ];

    let boundary = known(
        WorldBoundary::try_new(Some(-50.0), Some(500.0), None)
            .expect("the fixture boundary is well formed"),
        "fixture.boundary",
    );

    WorldDefinition::try_new(
        WorldId::from_key(WORLD_KEY).expect("the fixture world key is valid"),
        Origin::SyntheticFixture,
        boundary,
        sectors,
        objects,
        fixture_provenance("arch_world.record"),
    )
}

// -------------------------------------------------------------------- probe ---

/// Why a swept probe could not be spawned.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ProbeError {
    /// A named field was NaN or infinite.
    NonFinite {
        /// The offending field, as the caller spelled it.
        field: &'static str,
    },
    /// The mass was not strictly positive.
    NonPositiveMass,
    /// A box half extent was not strictly positive.
    NonPositiveHalfExtent {
        /// The axis whose half extent was zero or negative.
        axis: &'static str,
    },
}

impl std::fmt::Display for ProbeError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::NonFinite { field } => write!(f, "{field} must be finite"),
            Self::NonPositiveMass => write!(f, "mass_kg must be greater than zero"),
            Self::NonPositiveHalfExtent { axis } => {
                write!(f, "half_extents_m[{axis}] must be greater than zero")
            }
        }
    }
}

impl std::error::Error for ProbeError {}

/// The body the acceptance tests fly through (or into) the arch.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct ProbeSpec {
    /// Initial world position, in meters.
    pub position_m: [f64; 3],
    /// Initial world velocity, in m/s.
    pub velocity_m_s: [f64; 3],
    /// Half extents of the probe's box, in meters.
    pub half_extents_m: [f64; 3],
    /// Total mass, in kilograms.
    pub mass_kg: f64,
}

impl ProbeSpec {
    /// Validates the probe before any entity exists.
    ///
    /// # Errors
    ///
    /// [`ProbeError::NonFinite`], [`ProbeError::NonPositiveMass`] or
    /// [`ProbeError::NonPositiveHalfExtent`], naming the field.
    pub fn validate(&self) -> Result<(), ProbeError> {
        for (value, field) in self
            .position_m
            .iter()
            .chain(self.velocity_m_s.iter())
            .chain([self.mass_kg].iter())
            .copied()
            .zip([
                "position_m[0]",
                "position_m[1]",
                "position_m[2]",
                "velocity_m_s[0]",
                "velocity_m_s[1]",
                "velocity_m_s[2]",
                "mass_kg",
            ])
        {
            if !value.is_finite() {
                return Err(ProbeError::NonFinite { field });
            }
        }
        if self.mass_kg <= 0.0 {
            return Err(ProbeError::NonPositiveMass);
        }
        for (axis, extent) in ["x", "y", "z"].into_iter().zip(self.half_extents_m) {
            if !extent.is_finite() {
                return Err(ProbeError::NonFinite {
                    field: "half_extents_m",
                });
            }
            if extent <= 0.0 {
                return Err(ProbeError::NonPositiveHalfExtent { axis });
            }
        }
        Ok(())
    }
}

/// Spawns a dynamic, swept-CCD body on the aircraft collision layer.
///
/// The probe opts in to [`SweptCcd`] because the acceptance scenario *is* a
/// high-speed sweep, and it sets [`SpeculativeMargin::ZERO`] so that sweep is
/// what the test measures. Measured on the pinned pair (`avian 0.7.0`,
/// `bevy 0.19.1`, `SubstepCount(1)`): with Avian's *default* speculative
/// margin the probe stops at the leg even with `SweptCcd` removed, so a test
/// over the defaults cannot tell a swept sweep from speculative collision;
/// with the margin zeroed, removing `SweptCcd` makes the same probe tunnel
/// from `x = -1.83` straight past the wall with an empty contact log, and
/// restoring it clamps the probe to `x = -0.75` — exactly the wall's near
/// face — on the crossing tick. The fixture therefore isolates the swept
/// path rather than relying on whichever default the pinned version ships.
///
/// Whether retail aircraft bodies run continuous detection at all is
/// unmeasured and is not claimed here. Gravity, the fixed rate and the
/// one-tick integration are the ones [`crate::physics`] already owns, so the
/// probe travels at a constant velocity until something stops it.
///
/// # Errors
///
/// [`ProbeError`] when the spec is invalid; nothing is spawned then.
pub fn spawn_swept_probe(app: &mut App, spec: &ProbeSpec) -> Result<Entity, ProbeError> {
    spec.validate()?;
    let position = Vec3::new(
        spec.position_m[0] as f32,
        spec.position_m[1] as f32,
        spec.position_m[2] as f32,
    );
    let velocity = Vec3::new(
        spec.velocity_m_s[0] as f32,
        spec.velocity_m_s[1] as f32,
        spec.velocity_m_s[2] as f32,
    );
    Ok(app
        .world_mut()
        .spawn((
            RigidBody::Dynamic,
            Collider::cuboid(
                (spec.half_extents_m[0] * 2.0) as f32,
                (spec.half_extents_m[1] * 2.0) as f32,
                (spec.half_extents_m[2] * 2.0) as f32,
            ),
            Mass(spec.mass_kg as f32),
            Transform::from_translation(position),
            Position(position),
            Rotation::default(),
            LinearVelocity(velocity),
            AngularVelocity(Vec3::ZERO),
            SweptCcd::default(),
            SpeculativeMargin::ZERO,
            CollisionEventsEnabled,
            avian_layers(CollisionLayers::from(CollisionLayer::Aircraft)),
        ))
        .id())
}

/// The layer set a spawned probe carries; exported so a test can assert the
/// probe and the world were built from the same designed matrix.
#[must_use]
pub fn probe_layers() -> AvianCollisionLayers {
    avian_layers(CollisionLayers::from(CollisionLayer::Aircraft))
}

/// The layer set every static world collider carries.
#[must_use]
pub fn static_world_layers() -> AvianCollisionLayers {
    avian_layers(CollisionLayers::from(CollisionLayer::StaticWorld))
}

/// The set of objects in a load record's population, for tests and callers
/// that want an explicit subset.
///
/// A tiny helper so a caller does not have to reach into
/// [`cs_content::world::WorldPopulation`] with its own iterator plumbing.
#[must_use]
pub fn object_set(keys: &[&str]) -> BTreeSet<WorldObjectId> {
    keys.iter().map(|key| object(key)).collect()
}

// ---------------------------------------------------------------- harness ---

/// Why a [`WorldFixture`] could not be built.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum WorldFixtureError {
    /// The definition could not be spawned into the Bevy/Avian world.
    Spawn(super::spawn::WorldSpawnError),
    /// The swept probe's spec was invalid; nothing was spawned.
    Probe(ProbeError),
}

impl std::fmt::Display for WorldFixtureError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Spawn(err) => write!(f, "could not spawn the world: {err}"),
            Self::Probe(err) => write!(f, "could not spawn the probe: {err}"),
        }
    }
}

impl std::error::Error for WorldFixtureError {}

impl From<super::spawn::WorldSpawnError> for WorldFixtureError {
    fn from(err: super::spawn::WorldSpawnError) -> Self {
        Self::Spawn(err)
    }
}

impl From<ProbeError> for WorldFixtureError {
    fn from(err: ProbeError) -> Self {
        Self::Probe(err)
    }
}

/// Builds a [`WorldFixture`].
pub struct WorldFixtureBuilder {
    definition: WorldDefinition,
    probe: Option<ProbeSpec>,
}

impl WorldFixtureBuilder {
    /// Starts building a fixture for `definition`.
    #[must_use]
    pub fn new(definition: WorldDefinition) -> Self {
        Self {
            definition,
            probe: None,
        }
    }

    /// Also spawn a swept probe. Without one, the fixture is a static world.
    #[must_use]
    pub const fn probe(mut self, spec: ProbeSpec) -> Self {
        self.probe = Some(spec);
        self
    }

    /// Builds the fixture: the real pinned Bevy/Avian plugin group, the real
    /// F23-A fixed-rate adapter, gravity zero, and the definition spawned
    /// through [`super::spawn_world`].
    ///
    /// # Errors
    ///
    /// [`WorldFixtureError`] when the definition cannot be spawned or the
    /// probe's spec is invalid.
    pub fn build(self) -> Result<WorldFixture, WorldFixtureError> {
        let frame = Duration::from_secs_f64(1.0 / crate::physics::BASELINE_FIXED_HZ as f64);

        let mut app = App::new();
        app.add_plugins((MinimalPlugins, TransformPlugin, PhysicsPlugins::default()));
        app.add_plugins(crate::physics::PhysicsAdapterPlugin::new(
            crate::physics::BASELINE_FIXED_HZ,
        ));
        app.insert_resource(TimeUpdateStrategy::ManualDuration(frame));
        app.insert_resource(SubstepCount(1));
        app.insert_resource(Gravity::ZERO);

        // Seed the real clock baseline so the first counted update produces
        // the full manual delta and one fixed step; see
        // `docs/findings/2026-09-23-t334-first-frame-fixed-step.md`.
        let startup = app.world().resource::<Time<Real>>().startup();
        app.world_mut()
            .resource_mut::<Time<Real>>()
            .update_with_instant(startup);

        let spawned = super::spawn_world(&mut app, &self.definition)?;
        let probe = match self.probe {
            Some(spec) => Some(spawn_swept_probe(&mut app, &spec)?),
            None => None,
        };

        app.finish();
        app.cleanup();

        Ok(WorldFixture {
            app,
            definition: self.definition,
            spawned,
            probe,
            ticks: 0,
        })
    }
}

impl Default for WorldFixtureBuilder {
    /// A fixture for the synthetic arch world, with no probe.
    fn default() -> Self {
        Self::new(arch_world().expect("the synthetic arch world is well formed"))
    }
}

/// A headless world driven by the real fixed-rate physics adapter.
pub struct WorldFixture {
    app: App,
    definition: WorldDefinition,
    spawned: SpawnedWorld,
    probe: Option<Entity>,
    ticks: u64,
}

impl WorldFixture {
    /// Starts building a fixture for `definition`.
    #[must_use]
    pub fn builder(definition: WorldDefinition) -> WorldFixtureBuilder {
        WorldFixtureBuilder::new(definition)
    }

    /// A fixture for the synthetic arch world, with no probe.
    #[must_use]
    pub fn arch() -> WorldFixture {
        Self::builder(arch_world().expect("the synthetic arch world is well formed"))
            .build()
            .expect("the synthetic arch world spawns")
    }

    /// The definition that was spawned.
    #[must_use]
    pub fn definition(&self) -> &WorldDefinition {
        &self.definition
    }

    /// What [`super::spawn_world`] produced.
    #[must_use]
    pub fn spawned(&self) -> &SpawnedWorld {
        &self.spawned
    }

    /// The probe entity, when one was requested.
    #[must_use]
    pub fn probe(&self) -> Option<Entity> {
        self.probe
    }

    /// Read-only access to the Bevy world, for diagnostics and probes.
    #[must_use]
    pub fn world(&self) -> &bevy::prelude::World {
        self.app.world()
    }

    /// The probe's current position in meters, or `None` without a probe.
    #[must_use]
    pub fn probe_position(&self) -> Option<Vec3> {
        let probe = self.probe?;
        self.app.world().get::<Position>(probe).map(|p| p.0)
    }

    /// The probe's current linear velocity in m/s, or `None` without a probe.
    #[must_use]
    pub fn probe_velocity(&self) -> Option<Vec3> {
        let probe = self.probe?;
        self.app.world().get::<LinearVelocity>(probe).map(|v| v.0)
    }

    /// Every contact recorded so far.
    #[must_use]
    pub fn contacts(&self) -> &[super::contacts::WorldContact] {
        self.app
            .world()
            .resource::<super::contacts::WorldContacts>()
            .contacts()
    }

    /// The number of fixed ticks stepped so far.
    #[must_use]
    pub const fn ticks(&self) -> u64 {
        self.ticks
    }

    /// Advances the world by exactly `ticks` fixed steps.
    pub fn step(&mut self, ticks: u64) {
        for _ in 0..ticks {
            self.app.update();
            self.ticks += 1;
        }
    }
}
