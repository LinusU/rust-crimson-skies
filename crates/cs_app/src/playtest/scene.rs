//! The synthetic playtest scene: ground, one large obstacle and the aircraft.
//!
//! Every dimension here is newly authored for the development playtest and is
//! labelled as such on screen. Nothing is original geometry.
//!
//! The aircraft is the one thing that differs by scene: over original content
//! ([`RetailFlight`]) it flies the original game's recovered fixed-wing law
//! with the imported `pbloodhawk` parameters (task #797), and without it the
//! designed synthetic airframe below. Both enter the world through
//! [`crate::physics::spawn_body`], and both are marked [`PlaytestAircraft`].

use avian3d::prelude::{NoAutoMass, Rotation};
use bevy::prelude::{Component, Entity, Visibility, World};
use cs_sim::collision::CollisionLayer;
use cs_sim::flight::{
    EngineState, FlightInput, FlightModel, OriginalFlightModel, OriginalInput, OriginalState,
    synthetic_fixed_wing,
};
use cs_types::space::Quaternion;

use super::command::CRUISE_THROTTLE;
use super::propeller::PropellerSpin;
use super::retail::{RetailContent, RetailFlight};
use crate::physics::{
    BodyError, BodyMode, BodySpec, FlightSpawnError, FlightSpawnSpec, spawn_body, spawn_flight_body,
};

/// Where the aircraft starts, in meters (+Y up, forward is -Z).
pub const SPAWN_POSITION_M: [f32; 3] = [0.0, 250.0, 0.0];

/// The level cruise speed the synthetic fixed-wing declares, in m/s: the
/// synthetic scene's start speed.
///
/// The retail scene uses neither this number nor any synthetic tuning: it
/// starts at its own declared start speed (`retail::RETAIL_START_SPEED_M_S`)
/// and its cruise is the imported `fd_speed` the recovered law settles at.
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

/// The original 2000 PC game's fixed-wing law as the retail playtest flies it
/// (task #797): the recovered law, this airframe's imported parameters and the
/// command the input session holds.
///
/// The component is the per-aircraft record of the retail flight path, the
/// counterpart [`crate::physics::FlightAircraft`] is for the designed law. It
/// exists only on a body spawned over [`RetailContent`]; the synthetic scene
/// keeps [`FlightModel`](cs_sim::flight::FlightModel) and its own driver.
///
/// The law owns its own state ([`OriginalState`]) because it integrates pose
/// and velocity itself: the driver seeds it from the body each fixed tick,
/// hands the step's acceleration back to the body as one tick's force and
/// writes the attitude the step integrated (`docs/findings/
/// 2026-10-08-flight-original-fixed-wing-law.md`, "Deviations"). Nothing here
/// is `verified_original`; the provenance is [`Self::provenance`].
#[derive(Component, Clone, Debug)]
pub struct PlaytestOriginalFlight {
    /// The statically recovered law with this record's imported parameters.
    pub model: OriginalFlightModel,
    /// The law's own state: pose, velocity, angular momentum, throttle, fuel
    /// and the Level-Off toggle.
    pub state: OriginalState,
    /// The held command, written by the input producer once per frame.
    pub command: FlightInput,
    /// Hold the attitude while the forces still integrate: the original's own
    /// dev tool (`0x491c60`) measures a top speed and a climb this way, and
    /// [`crate::playtest`] flying an interactive session leaves it `false`.
    /// It is a field of the record so a test can fly the held-attitude probe
    /// the law documents instead of approximating it with stick input.
    pub hold_attitude: bool,
    /// `OWNER-STATIC-2026-10-08`: static analysis, never `verified_original`.
    pub provenance: &'static str,
}

impl PlaytestOriginalFlight {
    /// The record over original content, at `position_m` flying level at the
    /// playtest's start speed, with the throttle already at the cruise setting
    /// the session starts every aircraft at.
    ///
    /// The start speed is the scene's own declared value
    /// (`retail::RETAIL_START_SPEED_M_S`): **the original's player spawn speed
    /// was not recovered** (#796, unresolved until an original run, #358), so
    /// it is a documented development choice and not a measurement. The cruise
    /// the aircraft settles at under full throttle is the imported `fd_speed`
    /// of the record, which is data.
    #[must_use]
    pub fn new(retail: &RetailFlight, position_m: [f32; 3]) -> Self {
        let start_speed = retail.start_speed_m_s();
        Self {
            model: retail.model.clone(),
            state: OriginalState {
                position_m: position_m.map(f64::from),
                velocity_mps: [0.0, 0.0, -start_speed],
                orientation: Quaternion::IDENTITY,
                angular_momentum_world: [0.0; 3],
                throttle: f64::from(CRUISE_THROTTLE),
                fuel: retail.fuel,
                // The original's Level-Off toggle (Shift+L, command 47): it
                // starts off, and the playtest's `L` binding toggles it through
                // `FlightCommand::LevelOff` (#1134).
                level_off: false,
            },
            command: FlightInput {
                throttle: f64::from(CRUISE_THROTTLE),
                ..FlightInput::NEUTRAL
            },
            hold_attitude: false,
            provenance: retail.provenance,
        }
    }

    /// The engine the presentation systems read: the law's own **actual**
    /// (slewed) throttle is the spool, so the propeller follows the engine
    /// rather than the key, and an exhausted fuel load stops it.
    #[must_use]
    pub fn engine(&self) -> EngineState {
        EngineState {
            running: self.state.fuel > 0.0,
            spool: self.state.throttle,
        }
    }

    /// The held command as the original law reads it, already clamped by the
    /// law's own boundary. `is_player` selects the player blend and
    /// weathervane, [`Self::hold_attitude`] is the record's held-attitude
    /// probe, and `nitro` is the boost button — which the playtest's input
    /// layer never sets (`flight_command` passes `boost: false`), so nitro is
    /// unreachable here rather than invented.
    ///
    /// **Roll is negated.** [`FlightInput::roll`] is positive
    /// **right-wing-down** — the F22 action map's `E`, and the playtest's own
    /// declared contract — while the recovered law integrates a positive roll
    /// input as `delta L` about the body **+Z**, which is right-wing-*up*
    /// (measured here: holding `E` banked the retail aircraft −47°). The
    /// original's own roll axis sign was **not** recovered (#796 lists "roll
    /// sign" as an open unknown), so this negation makes the original law obey
    /// the project's control contract; it is not a claim about the original's
    /// axis. Pitch and yaw already agree with the contract and pass through.
    #[must_use]
    pub fn input(&self) -> OriginalInput {
        OriginalInput {
            roll: -self.command.roll,
            pitch: self.command.pitch,
            yaw: self.command.yaw,
            throttle: self.command.throttle,
            nitro: self.command.boost,
            engine_out: false,
            is_player: true,
            hold_attitude: self.hold_attitude,
        }
    }
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
/// flyable state: level, at the scene's start speed and cruise throttle,
/// heading -Z.
///
/// Over original content ([`RetailContent`]) the spawn point is the designed
/// pose in the area's frame and the collider is the box measured from the
/// aircraft's own mesh, and the body carries the original game's recovered
/// law with `pbloodhawk`'s imported parameters ([`RetailFlight`]) instead of
/// the synthetic fixed-wing. Without original content nothing changes: the
/// synthetic scene keeps [`synthetic_fixed_wing`].
pub fn spawn_aircraft(world: &mut World) -> Result<Entity, SceneError> {
    let (position, half_extents) = world
        .get_resource::<RetailContent>()
        .map_or((SPAWN_POSITION_M, AIRCRAFT_HALF_EXTENTS_M), |retail| {
            (retail.spawn_m, retail.half_extents_m)
        });
    let original = world
        .get_resource::<RetailFlight>()
        .map(|retail| PlaytestOriginalFlight::new(retail, position));
    let entity = match original {
        Some(flight) => spawn_original_flight_body(world, &flight, position, half_extents)?,
        None => spawn_synthetic_flight_body(world, position, half_extents)?,
    };
    // The aircraft is the render parent of its drawn parts; a parent without
    // `Visibility` makes Bevy warn (B0004) for every part on each (re)spawn. It
    // is set here, on the playtest's own flight body, rather than in
    // `spawn_body`, because only this body is a render parent: collision-only
    // bodies stay free of render components.
    world
        .entity_mut(entity)
        .insert((PlaytestAircraft, Visibility::Inherited));
    spawn_retail_parts(world, entity);
    debug_assert!(world.get::<Rotation>(entity).is_some());
    Ok(entity)
}

/// The synthetic scene's aircraft: the production [`spawn_flight_body`] path
/// with the designed fixed-wing, exactly as before original content had a law
/// of its own.
fn spawn_synthetic_flight_body(
    world: &mut World,
    position: [f32; 3],
    half_extents: [f32; 3],
) -> Result<Entity, SceneError> {
    let mut spec = FlightSpawnSpec::level_at(position, [0.0, 0.0, -SPAWN_SPEED_M_S]);
    spec.half_extents_m = half_extents;
    spec.engine = EngineState::direct(f64::from(CRUISE_THROTTLE));
    spec.command = FlightInput::try_new(0.0, 0.0, 0.0, f64::from(CRUISE_THROTTLE), false)
        .expect("the cruise command is in range");
    spawn_flight_body(world, FlightModel::new(synthetic_fixed_wing()), &spec)
        .map_err(SceneError::Aircraft)
}

/// The retail scene's aircraft: one dynamic body on the `Aircraft` layer whose
/// **declared mass is the imported weight over the law's own force scale**
/// (`W / 9.82`, #796's contract mapping), so the one-tick force the driver
/// submits accelerates it by exactly the acceleration the law computed.
///
/// Two things the law already owns are kept off the body on purpose: the
/// global [`avian3d::prelude::Gravity`] is `ZERO` (the step contains gravity
/// and drag itself, so a second one would double-count what the contract
/// forbids), and the body integrates no torque — the law rotates the attitude
/// itself, because the original integrates `2 * |omega| * dt` which no
/// torque-driven rigid body reproduces.
///
/// [`NoAutoMass`] is bound after the collider exists so the collider's derived
/// density can never replace the declared mass.
fn spawn_original_flight_body(
    world: &mut World,
    flight: &PlaytestOriginalFlight,
    position: [f32; 3],
    half_extents: [f32; 3],
) -> Result<Entity, SceneError> {
    let weight = flight.model.airframe.veh_weight;
    // `BodySpec::validate` refuses a non-positive mass by name, so a record
    // that cannot state one never spawns half a body.
    let mass_kg = (weight / cs_sim::flight::original::FORCE_TO_ACCEL) as f32;
    let start_speed = flight.state.velocity_mps;
    let spec = BodySpec {
        layer: CollisionLayer::Aircraft,
        shape: cs_sim::collision::ShapeClass::Solid,
        mode: BodyMode::Dynamic,
        mass_kg,
        half_extents_m: half_extents,
        position_m: position,
        linear_velocity_m_s: [
            start_speed[0] as f32,
            start_speed[1] as f32,
            start_speed[2] as f32,
        ],
    };
    let entity = spawn_body(world, &spec).map_err(SceneError::Body)?;
    world
        .entity_mut(entity)
        .insert((NoAutoMass, flight.clone()));
    Ok(entity)
}

/// Puts every drawn original mesh of the airframe under the flight body, one child
/// per mesh binding at its composed transform (with the designed nose mapping
/// applied once) and one drawn grandchild per stored material group, so the whole
/// aircraft moves as the one rigid body whose pose the flight path owns. A child carries no body, no collider and no pose of its own;
/// despawning the body (reset) despawns every part with it.
///
/// The one drawn propeller additionally carries [`PropellerSpin`], built from
/// the hub **measured from its own triangles**, so `R` reset re-spawns exactly
/// one spinning propeller with the fresh body and never leaves two.
fn spawn_retail_parts(world: &mut World, body: Entity) {
    let Some(retail) = world.get_resource::<RetailContent>() else {
        return;
    };
    let rotation = retail.visual_rotation;
    let parts = retail.parts.clone();
    let propeller = retail.propeller.clone();
    for part in &parts {
        let base = part.oriented(rotation);
        let entity = part.spawn(world, body, base);
        if let Some(spec) = &propeller
            && spec.node_slot == part.node_slot
        {
            world
                .entity_mut(entity)
                .insert(PropellerSpin::from_hub(&spec.hub, base));
        }
    }
}
