//! The synthetic playtest scene: ground, one large obstacle and the aircraft.
//!
//! Every dimension here is newly authored for the development playtest and is
//! labelled as such on screen. Nothing is original geometry.

use avian3d::prelude::Rotation;
use bevy::prelude::{ChildOf, Component, Entity, Mesh3d, MeshMaterial3d, World};
use cs_sim::collision::CollisionLayer;
use cs_sim::flight::{EngineState, FlightModel, synthetic_fixed_wing};

use super::retail::RetailContent;
use crate::physics::{
    BodyError, BodyMode, BodySpec, FlightSpawnError, FlightSpawnSpec, spawn_body, spawn_flight_body,
};
use crate::playtest_retail::AircraftPart;

/// Where the aircraft starts, in meters (+Y up, forward is -Z).
pub const SPAWN_POSITION_M: [f32; 3] = [0.0, 250.0, 0.0];

/// The level cruise speed the synthetic fixed-wing declares, in m/s.
pub const SPAWN_SPEED_M_S: f32 = 55.0;

/// Half extents of the aircraft's collider, in meters: a 12 m wingspan and a
/// 10 m fuselage.
pub const AIRCRAFT_HALF_EXTENTS_M: [f32; 3] = [6.0, 1.0, 5.0];

/// Centre of the large obstacle tower, 450 m ahead of the spawn point.
pub const OBSTACLE_CENTRE_M: [f32; 3] = [0.0, 200.0, -450.0];

/// Half extents of the obstacle tower: 120 m wide, 400 m tall, 50 m thick.
pub const OBSTACLE_HALF_EXTENTS_M: [f32; 3] = [60.0, 200.0, 25.0];

/// Half extents of the ground slab; its top face is at `y = 0`.
pub const GROUND_HALF_EXTENTS_M: [f32; 3] = [10_000.0, 5.0, 10_000.0];

/// Marks the one player aircraft.
#[derive(Component, Clone, Copy, Debug, Default)]
pub struct PlaytestAircraft;

/// Marks the obstacle wall; a contact with it is the observable collision.
#[derive(Component, Clone, Copy, Debug)]
pub struct PlaytestObstacle {
    /// Half extents the visual must match.
    pub half_extents_m: [f32; 3],
}

/// Marks the ground slab.
#[derive(Component, Clone, Copy, Debug)]
pub struct PlaytestGround {
    /// Half extents the visual must match.
    pub half_extents_m: [f32; 3],
}

/// Why the scene could not be built.
#[derive(Debug)]
pub enum SceneError {
    /// A static body was rejected.
    Body(BodyError),
    /// The aircraft was rejected.
    Aircraft(FlightSpawnError),
}

impl std::fmt::Display for SceneError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Body(error) => write!(f, "playtest scene body: {error}"),
            Self::Aircraft(error) => write!(f, "playtest aircraft: {error}"),
        }
    }
}

impl std::error::Error for SceneError {}

fn static_box(
    world: &mut World,
    layer: CollisionLayer,
    centre: [f32; 3],
    half: [f32; 3],
) -> Result<Entity, SceneError> {
    let spec = BodySpec {
        layer,
        shape: cs_sim::collision::ShapeClass::Solid,
        mode: BodyMode::Static,
        mass_kg: 0.0,
        half_extents_m: half,
        position_m: centre,
        linear_velocity_m_s: [0.0; 3],
    };
    spawn_body(world, &spec).map_err(SceneError::Body)
}

/// Spawns the ground and the obstacle once.
pub fn spawn_world(world: &mut World) -> Result<(), SceneError> {
    let ground = static_box(
        world,
        CollisionLayer::StaticWorld,
        [0.0, -GROUND_HALF_EXTENTS_M[1], 0.0],
        GROUND_HALF_EXTENTS_M,
    )?;
    world.entity_mut(ground).insert(PlaytestGround {
        half_extents_m: GROUND_HALF_EXTENTS_M,
    });
    let obstacle = static_box(
        world,
        CollisionLayer::StaticWorld,
        OBSTACLE_CENTRE_M,
        OBSTACLE_HALF_EXTENTS_M,
    )?;
    world.entity_mut(obstacle).insert(PlaytestObstacle {
        half_extents_m: OBSTACLE_HALF_EXTENTS_M,
    });
    Ok(())
}

/// Spawns the player aircraft through the production flight path in the known
/// flyable state: level, at cruise speed and cruise throttle, heading -Z.
///
/// Over original content ([`RetailContent`]) the spawn point is the designed
/// pose in the area's frame and the collider is the box measured from the
/// aircraft's own mesh; the flight model is still the synthetic fixed-wing.
pub fn spawn_aircraft(world: &mut World) -> Result<Entity, SceneError> {
    let (position, half_extents) = world
        .get_resource::<RetailContent>()
        .map_or((SPAWN_POSITION_M, AIRCRAFT_HALF_EXTENTS_M), |retail| {
            (retail.spawn_m, retail.half_extents_m)
        });
    let mut spec = FlightSpawnSpec::level_at(position, [0.0, 0.0, -SPAWN_SPEED_M_S]);
    spec.half_extents_m = half_extents;
    spec.engine = EngineState::direct(f64::from(super::command::CRUISE_THROTTLE));
    spec.command = cs_sim::flight::FlightInput::try_new(
        0.0,
        0.0,
        0.0,
        f64::from(super::command::CRUISE_THROTTLE),
        false,
    )
    .expect("the cruise command is in range");
    let entity = spawn_flight_body(world, FlightModel::new(synthetic_fixed_wing()), &spec)
        .map_err(SceneError::Aircraft)?;
    world.entity_mut(entity).insert(PlaytestAircraft);
    spawn_retail_parts(world, entity);
    debug_assert!(world.get::<Rotation>(entity).is_some());
    Ok(entity)
}

/// Puts every drawn original mesh of the airframe under the flight body, one child
/// per mesh binding at its composed transform (with the designed nose mapping
/// applied once), so the whole aircraft moves as the one rigid body whose pose the
/// flight path owns. A child carries no body, no collider and no pose of its own;
/// despawning the body (reset) despawns every part with it.
fn spawn_retail_parts(world: &mut World, body: Entity) {
    let Some(retail) = world.get_resource::<RetailContent>() else {
        return;
    };
    let rotation = retail.visual_rotation;
    let material = retail.material.clone();
    let parts: Vec<_> = retail
        .parts
        .iter()
        .map(|part| (part.mesh.clone(), part.oriented(rotation), part.node_slot))
        .collect();
    for (mesh, transform, node_slot) in parts {
        world.spawn((
            Mesh3d(mesh),
            MeshMaterial3d(material.clone()),
            transform,
            AircraftPart { node_slot },
            ChildOf(body),
        ));
    }
}
