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
//!   `Position` and `Transform` are moved to the contact point and the
//!   velocity component that carries it *into* the obstacle is removed
//!   (F23-D: a clamp alone is not a fix — see below). Sensor overlaps do not
//!   stop a spawn: the cast keeps the first hit the declared matrix calls a
//!   solid contact, which is the "apply damage once / sensor is not damage"
//!   boundary enforced at spawn, not after the fact;
//! * either way the correction is an [`SpawnPreflightEvent`] in
//!   [`SpawnPreflightLog`], the same authoritative-event channel the contact
//!   reports drain through, so a clamped spawn is a recorded gameplay fact
//!   and never a silent rewrite.
//!
//! A body that spawns at rest or whose layer never needed the sweep carries
//! no marker and is never cast.
//!
//! # Why the clamp also stops the body (F23-D)
//!
//! F23-C clamped the spawn *position* only and left `LinearVelocity` alone. The
//! F23-D contact probe measured what that does at the rates and speeds the
//! game uses: the clamp moves the body onto the contact, and then the very
//! tick it is about to run carries it a further `travel - distance` meters —
//! straight through the obstacle, because a body spawned this tick is still
//! invisible to the broad phase (F23-B limitation 1). Measured at 120 Hz: a
//! 10 cm projectile spawned 0.4 ticks (0.2 m) short of a 2 cm wall at 60 m/s
//! ended the tick at `x = +0.44`, sailed to `+3.94` and produced **no contact
//! report at all**; the same happened at 300/600 m/s, at every probed rate, and
//! with 2, 4 or 8 solver substeps. The engine's swept CCD and the substep
//! policy cannot help, because the pair is not in the collision detection
//! until the tick after the spawn.
//!
//! So the spawn's swept response is complete: the body lands on the contact
//! and its into-obstacle motion ends there. The tick that follows sees a
//! touching pair in the broad phase and reports the contact like any other
//! (AC02: exactly once, one tick later than an in-flight hit). The tangential
//! velocity is untouched, so a projectile that only grazes the wall keeps
//! sliding along it.
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

/// The overlap a clamped spawn keeps, in meters.
///
/// The swept layers run with `SpeculativeMargin::ZERO` (F23-B: an unbounded
/// speculative margin stops a fast body metres in front of an obstacle, which
/// is the hitbox inflation the spec forbids), and a zero margin means a pair
/// that only *touches* generates no contact. A spawn stopped exactly at the
/// time of impact would therefore be silent: measured at every probed rate and
/// speed, the projectile rested on the wall at `x = -0.0600` and the crossing
/// produced **no** contact report at all. This declared overlap — a tenth of
/// Avian's own 1 mm contact tolerance, and smaller than the thinnest obstacle
/// the contact probe covers — is what makes the following tick's narrow phase
/// see a real overlap, so the clamped spawn is reported exactly once like any
/// other crossing. The solver removes it over the following ticks.
pub const SPAWN_CONTACT_OVERLAP_M: f32 = 0.001;

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
    /// Whether the velocity component carrying the body into the obstacle was
    /// removed with the clamp. F23-D measured that a clamp without it still
    /// tunnels: the tick the clamp lands in carries the body through.
    pub stopped: bool,
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
    /// Preflights that also stopped a spawn's motion into an obstacle.
    stopped: u64,
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

    /// Preflights that also stopped a spawn's motion into an obstacle.
    pub fn stopped(&self) -> u64 {
        self.stopped
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
        self.stopped = 0;
    }

    fn record(&mut self, event: SpawnPreflightEvent) {
        self.total += 1;
        if event.clamped {
            self.clamped += 1;
        }
        if event.stopped {
            self.stopped += 1;
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
/// tick moves the body to the contact point and ends the velocity component
/// that carries it into the obstacle — writing `Position`/`LinearVelocity`
/// before the tick integrates is a teleport only in the bookkeeping sense:
/// the body has not simulated yet, so no continuity is broken.
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
                stopped: false,
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
                let clamped_to = position.0 + *direction * (hit.distance + SPAWN_CONTACT_OVERLAP_M);
                commands.entity(entity).insert((
                    Position(clamped_to),
                    Transform::from_translation(clamped_to).with_rotation(rotation.0),
                ));
                // The clamp alone is not enough (see the module docs): the
                // tick this body is about to run would carry it the rest of
                // the tick's travel, past the obstacle, while the pair is
                // still invisible to the broad phase. Ending the component of
                // the velocity that points into the obstacle is the swept
                // response for the spawn: the body stops on the contact and
                // the tick after this one reports it. Only the along-sweep
                // component is touched, so a graze keeps sliding.
                let into = velocity.0.dot(*direction);
                let stopped = into > 0.0;
                if stopped {
                    commands
                        .entity(entity)
                        .insert(LinearVelocity(velocity.0 - *direction * into));
                }
                log.record(SpawnPreflightEvent {
                    body: entity,
                    hit: Some(hit.entity),
                    distance_m: Some(hit.distance),
                    clamped: true,
                    stopped,
                });
            }
            None => {
                log.record(SpawnPreflightEvent {
                    body: entity,
                    hit: None,
                    distance_m: Some(travel),
                    clamped: false,
                    stopped: false,
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
