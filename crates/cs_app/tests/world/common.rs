//! Shared fixtures for the F18-A acceptance tests.
//!
//! Every value is authored here: synthetic geometry, speeds and probes. No
//! original game data and no `CS_GAME_DIR` access.

use avian3d::prelude::{Collider, ColliderAabb};
use bevy::prelude::{Entity, Mat4, Vec3, World};
use cs_app::world::{ProbeSpec, WorldFixture, arch_world, fixture::ARCH_LEG_Z_M};
use cs_content::scene::CanonicalTransform;
use cs_content::world::WorldDefinition;

/// The probe's box half extents, in meters.
pub const PROBE_HALF_M: f64 = 0.25;

/// The probe's speed along `+x`, in m/s. At the fixed rate this covers more
/// than three meters per tick — several times the arch's 1 m thickness — so
/// discrete position sampling alone cannot be trusted to notice the wall.
pub const PROBE_SPEED_M_S: f64 = 400.0;

/// Where the probe starts on the flight axis, in meters.
pub const PROBE_START_X_M: f64 = -28.5;

/// The probe's flight height, in meters: inside the opening (`y ∈ (0, 3)`).
pub const PROBE_Y_M: f64 = 1.5;

/// A probe flying straight down the `+x` axis at `z`.
///
/// The start is chosen so that the discrete tick positions *straddle* the
/// arch: consecutive samples land on either side of the 1 m thick leg with a
/// gap larger than probe plus wall, which is exactly the situation a non
/// swept test gets wrong.
#[must_use]
pub fn probe_at(z: f64) -> ProbeSpec {
    ProbeSpec {
        position_m: [PROBE_START_X_M, PROBE_Y_M, z],
        velocity_m_s: [PROBE_SPEED_M_S, 0.0, 0.0],
        half_extents_m: [PROBE_HALF_M, PROBE_HALF_M, PROBE_HALF_M],
        mass_kg: 250.0,
    }
}

/// The synthetic arch world, built by production code.
#[must_use]
pub fn arch() -> WorldDefinition {
    arch_world().expect("the synthetic arch world is well formed")
}

/// A world with a probe aimed through the opening (`z = 0`).
#[must_use]
pub fn through_opening() -> WorldFixture {
    WorldFixture::builder(arch())
        .probe(probe_at(0.0))
        .build()
        .expect("the arch fixture builds")
}

/// A world with a probe aimed straight at the right arch leg (`z = 1.5`).
#[must_use]
pub fn into_leg() -> WorldFixture {
    WorldFixture::builder(arch())
        .probe(probe_at(ARCH_LEG_Z_M))
        .build()
        .expect("the arch fixture builds")
}

/// The box a spawned collider *actually* uses, in its local frame.
///
/// Read from the collider's own scaled shape, so this is the geometry the
/// narrow phase resolves against — not an approximation of it.
#[must_use]
pub fn collider_box_half_extents(world: &World, entity: Entity) -> Vec3 {
    let collider = world
        .get::<Collider>(entity)
        .expect("the instance was spawned with a collider");
    let shape = collider
        .shape_scaled()
        .as_cuboid()
        .expect("the fixture only spawns box colliders");
    Vec3::new(
        shape.half_extents.x,
        shape.half_extents.y,
        shape.half_extents.z,
    )
}

/// The centre and half-size of Avian's broad-phase bound for a collider.
///
/// Avian inflates that bound by its contact margin, so a size comparison
/// must allow the margin while a centre comparison must not.
#[must_use]
pub fn collider_bounds(world: &World, entity: Entity) -> (Vec3, Vec3) {
    let bounds = world
        .get::<ColliderAabb>(entity)
        .expect("Avian computed a broad-phase bound for the collider");
    (
        (bounds.max + bounds.min) * 0.5,
        (bounds.max - bounds.min) * 0.5,
    )
}

/// The world-space axis-aligned bounds of one instance's authored box, in
/// meters: the authored canonical matrix applied to the eight local corners.
///
/// This is the reference a collider is measured against. It is computed from
/// the *record*, independently of the runtime decomposition, so a spawn that
/// places or scales a collider wrongly cannot agree with it.
#[must_use]
pub fn authored_aabb(transform: &CanonicalTransform, half_extents_m: [f64; 3]) -> (Vec3, Vec3) {
    let matrix: Mat4 = cs_app::world::canonical_matrix(transform);
    let mut min = Vec3::splat(f32::INFINITY);
    let mut max = Vec3::splat(f32::NEG_INFINITY);
    for sx in [-1.0_f32, 1.0] {
        for sy in [-1.0_f32, 1.0] {
            for sz in [-1.0_f32, 1.0] {
                let local = Vec3::new(
                    sx * half_extents_m[0] as f32,
                    sy * half_extents_m[1] as f32,
                    sz * half_extents_m[2] as f32,
                );
                let point = matrix.transform_point3(local);
                min = min.min(point);
                max = max.max(point);
            }
        }
    }
    (min, max)
}
