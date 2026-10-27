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
//!   [`DESIGNED_PLAYER_INTEGRITY`],
//!   so *which* contact kills is a design value, not the original's. The
//!   `health_damage_range` pair is converted to that pool's damage by
//!   [`crash_hull_damage`], calibrated on the owner's observation that a direct
//!   impact is fatal on first contact (#2005); the armor pair has no consumer.
//! * **Stops the simulation.** A fatal contact stamps the aircraft with a
//!   [`PlayerCrash`] record (tick, position, cause) in [`FatalContactRule::crash`], writes one [`PlayerCrashed`]
//!   message, zeroes the body's velocities and makes it static; the original
//!   flight law no longer steps that record. The pose stays owned by the one
//!   physics body.
//! * **Contacts are episodes.** Only the tick a contact pair starts is judged
//!   (the playtest's own contact episodes); the severity uses the velocity the
//!   body had at the end of the previous tick, because the solver has already
//!   answered the contact when the reports are read.

use avian3d::prelude::{
    AngularVelocity, Collisions, LinearVelocity, Position, RigidBody, Rotation,
};
use bevy::prelude::{
    Component, Entity, Message, MessageWriter, Or, Quat, Query, Res, ResMut, Resource, Vec3, With,
};
use cs_content::world::WorldCollisionRole;
use cs_sim::collision::CollisionLayer;
use cs_sim::contact_response::crash_damage;
use cs_sim::damage::{DamageChannel, DamageNodeKey, HitEvent};
use cs_types::Tick;
use cs_types::net::EventId;

use super::retail::PlaytestAreaBody;
use super::scene::{PlaytestAircraft, PlaytestGround, PlaytestSea};
use crate::airframe_damage::AirframeDamageRegion;
use crate::mission_session::{
    DESIGNED_PLAYER_INTEGRITY, MISSION_TARGET_NODE, PendingOrdnanceBursts, PlayerVictim,
    contact_wing_zone, queue_crash_smoke, wing_node,
};
use crate::physics::{ContactReports, PhysicsTickLedger};
use crate::weapons::SessionDamageResolver;
use crate::world::WorldColliderInstance;

/// The producer serial of the crash hits (the mission's 201..=205 are taken).
pub const MISSION_CRASH_PRODUCER: u32 = 206;

/// The raw crash damage (the doubled `crash` record damage, original units)
/// that empties an undamaged player hull in one contact: `300`, the damage a
/// contact of severity `s = 0.79` does (`2 * 300 * s^3`), a direct strike
/// about 38 degrees or less off the surface normal. **Designed, not measured**:
/// the owner's playtest of the original (2026-10-23, #2005) is that a direct,
/// non-glancing terrain impact is almost always fatal on first contact, and the
/// original's zone armor/health totals are still unrecovered (finding
/// `2026-10-22-crash-a-fatal-contact-and-crash-camera.md`).
pub const DESIGNED_LETHAL_CRASH_DAMAGE: f64 = 300.0;

/// How steeply the hull pool answers raw crash damage: the share of the pool
/// a contact takes is `(raw / DESIGNED_LETHAL_CRASH_DAMAGE)^2`. Designed. The
/// floor contacts of a sea skim (raw 100) cost a ninth of the pool, a hit at
/// 45 degrees (raw 216) about half, and one at or above the lethal damage all
/// of it; the weapons' damage to the same pool is untouched.
pub const DESIGNED_CRASH_HULL_EXPONENT: i32 = 2;

/// The damage a contact of raw crash damage `raw` does to the player's hull
/// pool of `integrity` (the designed [`DESIGNED_CRASH_HULL_EXPONENT`] curve,
/// see [`DESIGNED_LETHAL_CRASH_DAMAGE`]); the wing region takes `raw`.
#[must_use]
pub fn crash_hull_damage(raw: f64, integrity: f64) -> f64 {
    integrity * (raw / DESIGNED_LETHAL_CRASH_DAMAGE).powi(DESIGNED_CRASH_HULL_EXPONENT)
}

/// Component: the aircraft's terrain-contact state. Only a body that carries it
/// can crash; a reset despawns the aircraft and with it the state.
///
/// One component rather than three keeps the code the generic ECS machinery
/// instantiates small: the linked image is held under the unwind-table limit
/// (`accept_t336_`).
#[derive(Component, Clone, Copy, Debug, PartialEq)]
pub struct FatalContactRule {
    /// The `crash` record's `health_damage_range`: `[floor, scale]` of
    /// `max(scale * s^3, floor)`.
    pub health_damage_range: [f64; 2],
    /// The velocity the aircraft had at the end of the previous tick, the
    /// approach velocity of a contact the solver has already answered.
    pub approach: Vec3,
    /// Set from the fatal tick on.
    pub crash: Option<PlayerCrash>,
}

impl FatalContactRule {
    /// A rule with the given `health_damage_range`, not crashed.
    #[must_use]
    pub const fn new(health_damage_range: [f64; 2]) -> Self {
        Self {
            health_damage_range,
            approach: Vec3::ZERO,
            crash: None,
        }
    }
}

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

/// The record of a crash, kept in [`FatalContactRule::crash`].
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct PlayerCrash {
    /// The fixed tick of the fatal contact.
    pub tick: u64,
    /// The aircraft's position at that tick, metres.
    pub position_m: [f32; 3],
    /// Why.
    pub cause: FailureCause,
}

/// How far along the aircraft's heading the detached crash camera sits from
/// the impact point, world units (`camparam.zrd` `crash_horiz`, default
/// record; `OWNER-STATIC`, finding `2026-10-22-crash-a-fatal-contact-and-crash-camera.md`).
pub const CRASH_CAMERA_HORIZONTAL: f32 = 30.0;

/// How far above the impact point the detached crash camera sits
/// (`camparam.zrd` `crash_y`; the terrain clamp the original applies to the
/// anchor, `0x42c580`, is not decoded and not modelled).
pub const CRASH_CAMERA_HEIGHT: f32 = 45.0;

/// Where the target marker points after the crash: the owner's observation of
/// the original's placeholder target. Nothing in the engine image was found
/// that writes it (finding, section 4), so the point is an observation only.
pub const CRASH_PLACEHOLDER_TARGET: Vec3 = Vec3::new(1.0, 20.0, 0.0);

/// The string-table name of the `Fatal Crash!` message (id 162).
pub const FATAL_CRASH_MESSAGE: &str = "MSG_CRASH";

/// Resource: the sequence that follows a fatal crash. It holds the HUD and the
/// chase camera back, keeps the run open until a key is pressed and then
/// lets the mission host hand the run over to its debrief.
///
/// Inserted by [`judge_terrain_contacts`] on the fatal tick; a restart or a
/// teardown removes it.
#[derive(Resource, Clone, Copy, Debug, PartialEq)]
pub struct CrashSequence {
    /// The crash this sequence follows.
    pub crash: PlayerCrash,
    /// The fixed point the detached camera is held at.
    pub camera_m: Vec3,
    /// The placeholder target the marker points at.
    pub target_m: Vec3,
    /// A key was pressed: the run may settle.
    pub released: bool,
}

impl CrashSequence {
    /// The sequence of a crash of an aircraft turned by `rotation`.
    #[must_use]
    pub fn begin(crash: PlayerCrash, rotation: Quat) -> Self {
        Self {
            crash,
            camera_m: crash_camera_point(Vec3::from_array(crash.position_m), rotation),
            target_m: CRASH_PLACEHOLDER_TARGET,
            released: false,
        }
    }
}

/// The detached camera's point for a crash at `position` of an aircraft turned
/// by `rotation`: up by [`CRASH_CAMERA_HEIGHT`] and [`CRASH_CAMERA_HORIZONTAL`]
/// along the heading's horizontal direction.
#[must_use]
pub fn crash_camera_point(position: Vec3, rotation: Quat) -> Vec3 {
    let forward = rotation * Vec3::NEG_Z;
    let flat = Vec3::new(forward.x, 0.0, forward.z).normalize_or(Vec3::NEG_Z);
    position + flat * CRASH_CAMERA_HORIZONTAL + Vec3::Y * CRASH_CAMERA_HEIGHT
}

/// Message: the player crashed. One per fatal contact.
#[derive(Message, Clone, Copy, Debug, PartialEq)]
pub struct PlayerCrashed {
    /// The aircraft that crashed.
    pub aircraft: Entity,
    /// The record stamped on it.
    pub crash: PlayerCrash,
}

/// Message: one judged terrain contact episode of the player's aircraft, fatal
/// or not. One per contact that did damage.
#[derive(Message, Clone, Copy, Debug, PartialEq)]
pub struct TerrainContactJudged {
    /// The fixed tick of the contact.
    pub tick: u64,
    /// The aircraft that touched the terrain.
    pub aircraft: Entity,
    /// The world contact point, from the avian manifold point.
    pub point_m: Vec3,
    /// The unit surface normal, pointing out of the terrain.
    pub normal: Vec3,
    /// `-dot(normalize(v), n)`.
    pub severity: f64,
    /// The raw crash damage of the contact (original units), routed to the wing.
    pub damage: f64,
    /// The damage done to the hull pool ([`crash_hull_damage`] of `damage`).
    pub hull_damage: f64,
    /// The wing region the damage was routed to (designed side rule).
    pub wing: AirframeDamageRegion,
    /// The contact emptied the hull pool.
    pub fatal: bool,
}

/// The bodies a contact counts as terrain: the slab, the sea or the original
/// area's scenery, and the imported world's colliders — in a composed mission
/// the island, the plateau and the sea surface the aircraft bounces off all
/// live on one static `WorldColliderInstance` mesh body, which is the only
/// terrain a mission flight can reach (finding
/// `2026-10-23-crash-e-01-world-collider-is-terrain.md`).
///
/// A world collider counts only while its declared role is
/// [`WorldCollisionRole::Solid`]: a `Sensor` volume reports the overlap and
/// never blocks, so flying through one can never be a fatal terrain contact.
type TerrainFilter = Or<(
    With<PlaytestGround>,
    With<PlaytestSea>,
    With<PlaytestAreaBody>,
    With<WorldColliderInstance>,
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
    terrain: Query<Option<&WorldColliderInstance>, TerrainFilter>,
    mut aircraft: Query<
        (
            Entity,
            &Position,
            &Rotation,
            &mut LinearVelocity,
            &mut AngularVelocity,
            &mut FatalContactRule,
        ),
        With<PlaytestAircraft>,
    >,
    victim: Option<Res<PlayerVictim>>,
    damage: Option<ResMut<SessionDamageResolver>>,
    mut commands: bevy::prelude::Commands,
    mut crashed: MessageWriter<PlayerCrashed>,
    mut judged: MessageWriter<TerrainContactJudged>,
    mut smoke: Option<ResMut<PendingOrdnanceBursts>>,
) {
    let (Some(victim), Some(mut damage)) = (victim, damage) else {
        for (_, _, _, velocity, _, mut rule) in &mut aircraft {
            rule.approach = velocity.0;
        }
        return;
    };
    for (entity, position, rotation, mut velocity, mut angular, mut rule) in &mut aircraft {
        let before = rule.approach;
        rule.approach = velocity.0;
        if rule.crash.is_some() {
            continue;
        }
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
            let Ok(world_collider) = terrain.get(other) else {
                continue;
            };
            if world_collider.is_some_and(|c| c.role() != WorldCollisionRole::Solid) {
                continue;
            }
            let Some((normal, point)) = terrain_contact(&collisions, entity, other) else {
                continue;
            };
            let severity = contact_severity(before, normal);
            if severity <= 0.0 {
                continue;
            }
            let hit_damage = crash_damage(rule.health_damage_range, severity, true);
            let wing = contact_wing_zone(rotation.0.inverse() * (point - position.0)).value;
            let hull_damage = crash_hull_damage(hit_damage, DESIGNED_PLAYER_INTEGRITY);
            let hit_at = |sequence, node, amount| {
                HitEvent::try_new(
                    EventId {
                        session,
                        tick: Tick(ledger.ticks),
                        producer: MISSION_CRASH_PRODUCER,
                        sequence,
                    },
                    None,
                    victim.actor,
                    node,
                    DamageChannel::Internal,
                    amount,
                )
            };
            let (Ok(hit), Ok(wing_hit)) = (
                hit_at(sequence, node.clone(), hull_damage),
                hit_at(sequence + 1, wing_node(wing), hit_damage),
            ) else {
                continue;
            };
            sequence += 2;
            let was_destroyed = damage.resolver().is_destroyed(&victim.actor);
            if damage
                .resolver_mut()
                .resolve(Tick(ledger.ticks), &[hit, wing_hit])
                .is_err()
            {
                continue;
            }
            let fatal = !was_destroyed && damage.resolver().is_destroyed(&victim.actor);
            judged.write(TerrainContactJudged {
                tick: ledger.ticks,
                aircraft: entity,
                point_m: point,
                normal,
                severity,
                damage: hit_damage,
                hull_damage,
                wing,
                fatal,
            });
            if !fatal {
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
            velocity.0 = Vec3::ZERO;
            angular.0 = Vec3::ZERO;
            rule.crash = Some(crash);
            commands.entity(entity).insert(RigidBody::Static);
            commands.insert_resource(CrashSequence::begin(crash, rotation.0));
            if let Some(pending) = smoke.as_deref_mut() {
                queue_crash_smoke(pending, ledger.ticks, position.0);
            }
            crashed.write(PlayerCrashed {
                aircraft: entity,
                crash,
            });
            break;
        }
    }
}

/// The unit normal of the surface `other` presents to `aircraft`, pointing out
/// of the surface, and the world contact point, from the first touching
/// manifold of their pair.
fn terrain_contact(
    collisions: &Collisions,
    aircraft: Entity,
    other: Entity,
) -> Option<(Vec3, Vec3)> {
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
        Some((manifold.normal * sign, manifold.points.first()?.point))
    })
}
