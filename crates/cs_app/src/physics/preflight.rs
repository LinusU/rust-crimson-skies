//! First-tick spawn preflight for the swept layers (F23-C).
//!
//! Spec: `specs/F23-avian-integration-collision-and-fixed-step-authority.md`,
//! stage `### F23-C`. Shared contract: `docs/contracts/FLIGHT-PHYSICS.md`.
//!
//! F23-B measured a hole in Avian 0.7's swept detection: the swept AABB a
//! [`SweptCcd`] body uses is written at the *end* of a tick, so a fast body
//! spawned inside one tick's travel of an obstacle tunnels on its very first
//! tick — detection is only guaranteed from its second tick on
//! (`docs/findings/2026-09-30-f23-b-body-creation-forces-sweeps-and-transitions.md`,
//! limitation 1).
//!
//! This module is the producer-side repair. A body spawned through the
//! session ([`crate::physics::PhysicsSession::spawn`]) that needs swept
//! detection and is already moving carries the [`SpawnPreflight`] marker;
//! [`resolve_spawn_preflights`] runs in `FixedPostUpdate` **before**
//! `PhysicsSystems::Prepare` — the same verified pre-step slot the force
//! drain uses — and shape-casts the collider over exactly one tick of its
//! velocity, before the body has moved:
//!
//! * if the closest *solid* hit lands inside the tick's travel, the body's
//!   `Position` and `Transform` are moved to the contact point while
//!   `LinearVelocity` is untouched, so the tick resolves the contact instead
//!   of tunnelling through it. Sensor overlaps do not stop a spawn: the cast
//!   keeps the first hit the declared matrix calls a solid contact, which is
//!   the "apply damage once / sensor is not damage" boundary enforced at
//!   spawn, not after the fact;
//! * either way the correction is an [`SpawnPreflightEvent`] in
//!   [`SpawnPreflightLog`], the same authoritative-event channel the contact
//!   reports drain through, so a clamped spawn is a recorded gameplay fact
//!   and never a silent rewrite.
//!
//! A body that spawns at rest or whose layer never needed the sweep carries
//! no marker and is never cast.
//!
//! **Designed rule, not original data.** Whether the original clamped a
//! spawn, refused it or let it tunnel is unknown; this is the declared fix
//! for the measured engine hole.

use avian3d::prelude::{
    Collider, LinearVelocity, PhysicsSystems, Position, Rotation, Sensor, ShapeCastConfig,
    SpatialQuery, SpatialQueryFilter,
};
use bevy::{
    ecs::schedule::IntoScheduleConfigs,
    prelude::{
        App, Commands, Component, Dir3, Entity, FixedPostUpdate, Query, Res, ResMut, Resource,
        Transform, With,
    },
    time::{Fixed, Time},
};
use cs_sim::collision::{ContactKind, ShapeClass, classify_contact};

use super::body::BodyLayer;

/// Marks a just-spawned swept body whose first tick needs a preflight sweep.
///
/// The marker carries nothing: the cast re-derives distance from the body's
/// own [`LinearVelocity`] and the tick's own timestep, so a spawned entity
/// that changes hands before its first step still preflights against its
/// final velocity. It is removed when the cast resolves.
#[derive(Component, Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct SpawnPreflight;

/// One preflight decision, recorded authoritatively.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct SpawnPreflightEvent {
    /// The spawned body that was cast.
    pub body: Entity,
    /// The solid body the cast hit, if any.
    pub hit: Option<Entity>,
    /// How far the body could travel before the hit, in meters — less than
    /// one tick's travel exactly when the spawn was clamped.
    pub distance_m: Option<f32>,
    /// Whether the spawn position was moved to the contact point.
    pub clamped: bool,
}

/// The recorded preflight events of a running world.
///
/// A drain-what-you-read buffer in the shape of
/// [`ContactReports`](crate::physics::ContactReports): each entry is appended
/// once per fresh body and a consumer takes the whole batch, so a clamp can
/// never fire twice and is never silently dropped.
#[derive(Resource, Debug, Default)]
pub struct SpawnPreflightLog {
    events: Vec<SpawnPreflightEvent>,
    /// Preflights resolved since the log was built or cleared.
    total: u64,
    /// Preflights that clamped a spawn since the log was built or cleared.
    clamped: u64,
}

impl SpawnPreflightLog {
    /// The undrained events.
    pub fn events(&self) -> &[SpawnPreflightEvent] {
        &self.events
    }

    /// Preflights resolved since build or `clear`.
    pub fn total(&self) -> u64 {
        self.total
    }

    /// Preflights that moved a spawn since build or `clear`.
    pub fn clamped(&self) -> u64 {
        self.clamped
    }

    /// Takes every recorded event, leaving the counters.
    pub fn take(&mut self) -> Vec<SpawnPreflightEvent> {
        core::mem::take(&mut self.events)
    }

    /// Drops events and counters.
    pub fn clear(&mut self) {
        self.events.clear();
        self.total = 0;
        self.clamped = 0;
    }

    fn record(&mut self, event: SpawnPreflightEvent) {
        self.total += 1;
        if event.clamped {
            self.clamped += 1;
        }
        self.events.push(event);
    }
}

/// A fresh swept body and the components the preflight needs.
type FreshSweep<'w> = (
    Entity,
    &'w Collider,
    &'w BodyLayer,
    &'w LinearVelocity,
    &'w Rotation,
    &'w Position,
);

/// Resolves every [`SpawnPreflight`] marker against the current collider
/// tree, before the tick integrates.
///
/// For each fresh swept body it casts the body's own collider along one tick
/// of linear travel. The cast's predicate accepts only a hit the declared
/// matrix would resolve as a solid contact, so the closest sensor or
/// non-interacting collider never stops a spawn. A solid hit inside the
/// tick moves the body to the contact point — writing `Position` before the
/// tick integrates is a teleport only in the bookkeeping sense: the body has
/// not simulated yet, so no continuity is broken.
fn resolve_spawn_preflights(
    time: Res<Time<Fixed>>,
    spatial: SpatialQuery,
    movers: Query<FreshSweep<'_>, With<SpawnPreflight>>,
    layers: Query<&BodyLayer>,
    sensors: Query<(), With<Sensor>>,
    mut log: ResMut<SpawnPreflightLog>,
    mut commands: Commands,
) {
    let dt = time.timestep().as_secs_f32();
    for (entity, collider, layer, velocity, rotation, position) in movers.iter() {
        commands.entity(entity).remove::<SpawnPreflight>();

        let direction = velocity.0 * dt;
        let travel = direction.length();
        if travel == 0.0 {
            log.record(SpawnPreflightEvent {
                body: entity,
                hit: None,
                distance_m: None,
                clamped: false,
            });
            continue;
        }

        let direction = Dir3::new(direction).expect("a nonzero travel has a direction");
        let mover = layer.0;
        let filter = SpatialQueryFilter::from_mask(u32::from(mover.designed_partners().bits()))
            .with_excluded_entities([entity]);
        let config = ShapeCastConfig::from_max_distance(travel);
        let hit = spatial.cast_shape_predicate(
            collider,
            position.0,
            rotation.0,
            direction,
            &config,
            &filter,
            &|hit_entity| {
                // Only a solid contact may stop a spawn: an overlap the
                // matrix ignores, and every sensor, lets the cast pass on to
                // a later solid hit (contract: sensor overlap is not damage
                // by itself).
                let Ok(hit_layer) = layers.get(hit_entity) else {
                    return true;
                };
                let hit_shape = if sensors.contains(hit_entity) {
                    ShapeClass::Sensor
                } else {
                    ShapeClass::Solid
                };
                classify_contact(mover, hit_layer.0, ShapeClass::Solid, hit_shape)
                    == ContactKind::SolidContact
            },
        );

        match hit {
            Some(hit) => {
                // The clamp is deferred into the automatic sync point ahead
                // of `PhysicsSystems::Prepare`, so it lands before the tick
                // integrates — a teleport in the bookkeeping sense only: the
                // body has not simulated yet, so no continuity is broken.
                let clamped_to = position.0 + *direction * hit.distance;
                commands.entity(entity).insert((
                    Position(clamped_to),
                    Transform::from_translation(clamped_to).with_rotation(rotation.0),
                ));
                log.record(SpawnPreflightEvent {
                    body: entity,
                    hit: Some(hit.entity),
                    distance_m: Some(hit.distance),
                    clamped: true,
                });
            }
            None => {
                log.record(SpawnPreflightEvent {
                    body: entity,
                    hit: None,
                    distance_m: Some(travel),
                    clamped: false,
                });
            }
        }
    }
}

/// Installs the spawn-preflight channel.
///
/// The system is registered by [`PhysicsBodiesPlugin`](crate::physics::PhysicsBodiesPlugin);
/// this module owns the resource and the system, the plugin owns the
/// schedule slot — the same `FixedPostUpdate`, before
/// `PhysicsSystems::Prepare`, where the force drain already runs.
pub(crate) fn install(app: &mut App) {
    app.init_resource::<SpawnPreflightLog>();
    app.add_systems(
        FixedPostUpdate,
        resolve_spawn_preflights.before(PhysicsSystems::Prepare),
    );
}
