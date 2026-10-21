//! The fatal terrain contact (PARITY4-CRASH-B, #1854).
//!
//! The rule is the one `docs/findings/2026-10-22-crash-a-fatal-contact-and-crash-camera.md`
//! read out of the original (`OWNER-STATIC`, nothing `verified_original`): there
//! is no separate crash test on speed or angle. A terrain contact of severity
//! `s = -dot(normalize(v), n)` does the damage `max(range[1] * s^3, range[0])`,
//! doubled for the player ([`cs_sim::contact_response::crash_damage`]), and the
//! contact is fatal only because that damage empties the airframe's health.
//!
//! * **Damage** goes through the session's one damage authority, as a hit on the
//!   player's hull node with no attacker, so a crash and gunfire wear the same
//!   pool and the mission host's own `player_destroyed` rule (#1654) ends the
//!   run as a failure.
//! * **The pool is not measured.** The original's per-zone armor/health totals
//!   are unknown (finding, "Unknown"); the hull pool is the designed
//!   [`DESIGNED_PLAYER_INTEGRITY`](crate::mission_session::DESIGNED_PLAYER_INTEGRITY),
//!   so *which* contact kills is a design value, not the original's. The
//!   `health_damage_range` pair is applied as that pool's damage; the armor pair
//!   has no consumer.
//! * **Stops the simulation.** A fatal contact stamps the aircraft with a
//!   [`PlayerCrash`] record (tick, position, cause), writes one [`PlayerCrashed`]
//!   message, zeroes the body's velocities and makes it static; the original
//!   flight law no longer steps that record. The pose stays owned by the one
//!   physics body.
//! * **Contacts are episodes.** Only the tick a contact pair starts is judged
//!   (the playtest's own contact episodes); the severity uses the velocity the
//!   body had at the end of the previous tick, because the solver has already
//!   answered the contact when the reports are read.

use avian3d::prelude::{AngularVelocity, Collisions, LinearVelocity, Position, RigidBody};
use bevy::prelude::{
    Component, Entity, Message, MessageWriter, Or, Query, Res, ResMut, Vec3, With,
};
use cs_sim::collision::CollisionLayer;
use cs_sim::contact_response::crash_damage;
use cs_sim::damage::{DamageChannel, DamageNodeKey, HitEvent};
use cs_types::Tick;
use cs_types::net::EventId;

use super::retail::PlaytestAreaBody;
use super::scene::{PlaytestAircraft, PlaytestGround, PlaytestSea};
use crate::mission_session::{MISSION_TARGET_NODE, PlayerVictim};
use crate::physics::{ContactReports, PhysicsTickLedger};
use crate::weapons::SessionDamageResolver;

/// The producer serial of the crash hits (the mission's 201..=205 are taken).
pub const MISSION_CRASH_PRODUCER: u32 = 206;

/// Component: the `crash` record's `health_damage_range`, the aircraft's
/// terrain-contact damage rule. Only a body that carries it can crash.
#[derive(Component, Clone, Copy, Debug, PartialEq)]
pub struct FatalContactRule {
    /// `health_damage_range`: `[floor, scale]` of `max(scale * s^3, floor)`.
    pub health_damage_range: [f64; 2],
}

/// Component: the velocity the aircraft had at the end of the previous tick,
/// the approach velocity of a contact the solver has already answered.
#[derive(Component, Clone, Copy, Debug, Default, PartialEq)]
pub struct ContactApproach(pub Vec3);

/// Why the player's flight ended.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum FailureCause {
    /// A terrain contact emptied the airframe's health.
    TerrainContact {
        /// The terrain body that was hit.
        terrain: Entity,
        /// `-dot(normalize(v), n)` of the fatal contact.
        severity: f64,
        /// The damage the contact did to the hull pool.
        damage: f64,
    },
}

/// Component: the aircraft crashed. Present on the aircraft entity from the
/// fatal tick on; a reset despawns the aircraft and with it the record.
#[derive(Component, Clone, Copy, Debug, PartialEq)]
pub struct PlayerCrash {
    /// The fixed tick of the fatal contact.
    pub tick: u64,
    /// The aircraft's position at that tick, metres.
    pub position_m: [f32; 3],
    /// Why.
    pub cause: FailureCause,
}

/// Message: the player crashed. One per fatal contact.
#[derive(Message, Clone, Copy, Debug, PartialEq)]
pub struct PlayerCrashed {
    /// The aircraft that crashed.
    pub aircraft: Entity,
    /// The record stamped on it.
    pub crash: PlayerCrash,
}

/// The bodies a contact counts as terrain: the slab, the sea or the original
/// area's scenery.
type TerrainFilter = Or<(
    With<PlaytestGround>,
    With<PlaytestSea>,
    With<PlaytestAreaBody>,
)>;

/// `-dot(normalize(velocity), normal)`, 0 for a body at rest.
#[must_use]
pub fn contact_severity(velocity: Vec3, normal: Vec3) -> f64 {
    let speed = velocity.length();
    if speed <= f32::EPSILON {
        return 0.0;
    }
    f64::from(-(velocity / speed).dot(normal))
}

/// Judges this tick's new terrain contacts of the player's aircraft.
#[allow(clippy::too_many_arguments, clippy::type_complexity)]
pub fn judge_terrain_contacts(
    reports: Res<ContactReports>,
    ledger: Res<PhysicsTickLedger>,
    collisions: Collisions,
    terrain: Query<(), TerrainFilter>,
    mut aircraft: Query<
        (
            Entity,
            &Position,
            &LinearVelocity,
            &mut ContactApproach,
            Option<&FatalContactRule>,
        ),
        (With<PlaytestAircraft>, bevy::prelude::Without<PlayerCrash>),
    >,
    victim: Option<Res<PlayerVictim>>,
    damage: Option<ResMut<SessionDamageResolver>>,
    mut commands: bevy::prelude::Commands,
    mut crashed: MessageWriter<PlayerCrashed>,
) {
    let (Some(victim), Some(mut damage)) = (victim, damage) else {
        for (_, _, velocity, mut approach, _) in &mut aircraft {
            approach.0 = velocity.0;
        }
        return;
    };
    for (entity, position, velocity, mut approach, rule) in &mut aircraft {
        let before = approach.0;
        approach.0 = velocity.0;
        let Some(rule) = rule else { continue };
        let session = damage.resolver().session();
        let node = DamageNodeKey::new(MISSION_TARGET_NODE).expect("the hull key is valid");
        let mut sequence = 0_u32;
        for report in reports.reports() {
            if report.tick != ledger.ticks
                || !report.involves(CollisionLayer::Aircraft)
                || !report.bodies.contains(&entity)
            {
                continue;
            }
            let Some(other) = report.bodies.into_iter().find(|body| *body != entity) else {
                continue;
            };
            if !terrain.contains(other) {
                continue;
            }
            let Some(normal) = outward_normal(&collisions, entity, other) else {
                continue;
            };
            let severity = contact_severity(before, normal);
            if severity <= 0.0 {
                continue;
            }
            let hit_damage = crash_damage(rule.health_damage_range, severity, true);
            let Ok(hit) = HitEvent::try_new(
                EventId {
                    session,
                    tick: Tick(ledger.ticks),
                    producer: MISSION_CRASH_PRODUCER,
                    sequence,
                },
                None,
                victim.actor,
                node.clone(),
                DamageChannel::Internal,
                hit_damage,
            ) else {
                continue;
            };
            sequence += 1;
            let was_destroyed = damage.resolver().is_destroyed(&victim.actor);
            if damage
                .resolver_mut()
                .resolve(Tick(ledger.ticks), &[hit])
                .is_err()
            {
                continue;
            }
            if was_destroyed || !damage.resolver().is_destroyed(&victim.actor) {
                continue;
            }
            let crash = PlayerCrash {
                tick: ledger.ticks,
                position_m: position.0.to_array(),
                cause: FailureCause::TerrainContact {
                    terrain: other,
                    severity,
                    damage: hit_damage,
                },
            };
            commands.entity(entity).insert((
                crash,
                LinearVelocity::ZERO,
                AngularVelocity::ZERO,
                RigidBody::Static,
            ));
            crashed.write(PlayerCrashed {
                aircraft: entity,
                crash,
            });
            break;
        }
    }
}

/// The unit normal of the surface `other` presents to `aircraft`, pointing out
/// of the surface, from the first touching manifold of their pair.
fn outward_normal(collisions: &Collisions, aircraft: Entity, other: Entity) -> Option<Vec3> {
    use avian3d::prelude::ContactPairFlags;
    collisions.iter().find_map(|pair| {
        if !pair.flags.contains(ContactPairFlags::TOUCHING) {
            return None;
        }
        let aircraft_first = pair.body1 == Some(aircraft) || pair.collider1 == aircraft;
        let aircraft_second = pair.body2 == Some(aircraft) || pair.collider2 == aircraft;
        let other_first = pair.body1 == Some(other) || pair.collider1 == other;
        let other_second = pair.body2 == Some(other) || pair.collider2 == other;
        let sign = if aircraft_first && other_second {
            -1.0
        } else if aircraft_second && other_first {
            1.0
        } else {
            return None;
        };
        let manifold = pair.manifolds.iter().find(|m| !m.points.is_empty())?;
        Some(manifold.normal * sign)
    })
}
