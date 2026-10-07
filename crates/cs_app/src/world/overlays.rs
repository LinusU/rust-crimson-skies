//! Mission overlays: the runtime that fires a load's declared overlays and
//! applies their effect to both consumers of an object (F18-C).
//!
//! Spec: `specs/F18-world-geometry-terrain-water-and-traversable-interiors.md`,
//! stage `### F18-C`, acceptance scenario **AC03** — *open an authored door and
//! verify both render and collision update once.* Shared contract:
//! `docs/contracts/IDENTITY-CONTENT.md`.
//!
//! # The producer and the consumer
//!
//! An overlay is not a special entity and not a special system: it is an
//! ordinary authored object ([`cs_content::world::MissionOverlay`]) whose
//! trigger is a [`WorldCollisionRole::Sensor`] volume in the same
//! [`WorldDefinition`](cs_content::world::WorldDefinition) as everything else,
//! and the two ends of it are ordinary ECS wiring:
//!
//! | end | what it is |
//! | --- | --- |
//! | producer | [`queue_overlay_triggers`]: Avian's [`CollisionStart`] stream, read in `FixedPostUpdate` after the step, naming every sensor volume a body reached |
//! | hand-off | [`OverlayTriggerRequests`]: the trigger objects those contacts named, in stable order |
//! | consumer | [`apply_overlay_requests`], the exclusive pass that drains the hand-off and calls [`apply_overlay`] for each |
//! | trace | [`WorldOverlayLog`]: what was applied, and every refusal with its reason |
//!
//! The hand-off resource is deliberate rather than incidental. The producer runs
//! in a parallel schedule and cannot write a world record; the consumer needs
//! `&mut World` because applying an effect moves entities. A resource between
//! them is the shape F11-C's [`crate::scene::AirframeSceneRequest`] already
//! uses, it is inspectable from a test without stepping the schedule, and it
//! means a producer that fires for a world with no load cannot panic: the
//! consumer simply finds no resident world and records the refusal.
//!
//! # Why the effect moves *every* entity the object owns
//!
//! F18 non-negotiable behavior 1 makes visual and collision geometry share
//! provenance. That is only half the claim: an effect that moved the drawn
//! panel and left its collider where it was would satisfy "they share
//! provenance" and still be a solid door nobody can fly through — a mismatch
//! between the render and the collision that no amount of shared derivation
//! prevents. So [`displace_object`] writes the offset into **every** transform
//! the object's entities carry ([`Transform`] and Avian's [`Position`]) and
//! refuses an entity it cannot reach, rather than moving the half it happens to
//! find. For a cuboid object that is two entities — a presentation marker and a
//! collider — and for a mesh object it is the one entity that is both, so the two
//! layouts are the same code path.
//!
//! # "Once" is a property of the load
//!
//! [`apply_overlay`] refuses a trigger this load has already applied
//! ([`OverlayError::AlreadyApplied`]) and records nothing on a refusal, so a
//! body that crosses the same volume on every pass opens the door exactly once.
//! A failed application is *not* recorded, which is the retry: a target streamed
//! away ([`OverlayError::TargetNotPresent`]) or a despawned entity
//! ([`OverlayError::VanishedEntity`]) leaves the overlay pending, and the next
//! contact — or the caller — tries again. Because the applied set lives in
//! [`ResidentWorld`](super::residency::ResidentWorld) and not on an entity, it
//! survives the target's own sector being streamed away and unloaded: the
//! sector load re-applies it to the fresh entities
//! ([`super::residency::load_sector`]), and the door is still open. An
//! [`unload_world`](super::residency::unload_world) takes it with everything
//! else, so the next load's door is shut and its overlay unapplied.
//!
//! # What is **not** claimed about an original mission's triggers
//!
//! This layer is written against an **authored** trigger: a
//! [`WorldCollisionRole::Sensor`] cuboid in the same
//! [`WorldDefinition`](cs_content::world::WorldDefinition) as everything else,
//! sized by whoever authored the load. Task #427 measured what the original's
//! own triggers look like, and two things about them must not be read as though
//! this layer had measured them:
//!
//! * **No claim is made that a mission's trigger opens its geometry before a
//!   fast aircraft reaches it.** The effect lands in the update *after* the
//!   contact that triggered it, so a trigger placed closer to the geometry it
//!   opens than one tick of travel at the body's speed is crossed first, and the
//!   body meets the closed panel. The depot fixture's panel is 4 m ahead of its
//!   1 m trigger — 1.2 ticks at 400 m/s — and the fixture test says so rather
//!   than reading the resulting stop as a hold.
//! * **No claim is made that an original trigger is a thin authored box.** Over
//!   the owner's installation the original's own detection zones are world nodes
//!   (`dzpath<N>`) whose stored extent is **32 to 860 stored units** on their
//!   thinnest axis — 32 to 860 metres at the measured GameZ unit (task #677),
//!   though the trigger survey itself still carries no factor (task #733) —
//!   so they are large regions of the world, not sheets a sample can step
//!   over. Which of them a given mission uses, and what the original does when
//!   one is entered, are unmeasured. [`super::triggers`] is the measurement,
//!   and `docs/findings/2026-10-02-t427-retail-trigger-volume-thickness.md` is
//!   its record.
//!
//! So the discretely-reported overlap this producer consumes is the mechanism
//! **this project** uses for the volumes **this project** authors. Whether the
//! original detected its own zones that way is F13/F39's open question, and an
//! overlay bound to an original zone must not assume it was.

use std::collections::BTreeSet;

use avian3d::prelude::{CollisionStart, PhysicsSystems, Position};
use bevy::ecs::schedule::IntoScheduleConfigs;
use bevy::prelude::{
    App, Entity, FixedPostUpdate, MessageReader, Plugin, Query, ResMut, Resource, Transform, Vec3,
};
use cs_content::world::{OverlayEffect, WorldCollisionRole, WorldObjectId};

use super::contacts::{WorldColliderInstance, WorldObjectBinding};
use super::residency::{ResidentWorld, WorldResidency, residency};
use super::spawn::SpawnedObject;

/// The trigger objects whose overlays the mission's contacts have asked for.
///
/// One entry per trigger however many bodies reached it, and in stable order,
/// so a pass that applies them cannot apply the same overlay twice because two
/// bodies crossed the volume on the same tick.
#[derive(Resource, Default, Debug, Clone)]
pub struct OverlayTriggerRequests {
    triggers: BTreeSet<WorldObjectId>,
}

impl OverlayTriggerRequests {
    /// Asks for the overlay `trigger` to be applied on the next consumer pass.
    ///
    /// A repeated request is the same request: the volume is still there and the
    /// load still declares it, so there is nothing new to say.
    pub fn request(&mut self, trigger: WorldObjectId) {
        self.triggers.insert(trigger);
    }

    /// The triggers asked for, in stable order.
    #[must_use]
    pub fn triggers(&self) -> &BTreeSet<WorldObjectId> {
        &self.triggers
    }

    /// Whether a trigger is pending.
    #[must_use]
    pub fn is_requested(&self, trigger: &WorldObjectId) -> bool {
        self.triggers.contains(trigger)
    }

    /// Takes every pending request, leaving none behind.
    pub fn drain(&mut self) -> BTreeSet<WorldObjectId> {
        std::mem::take(&mut self.triggers)
    }
}

/// What one application of an overlay changed.
#[derive(Clone, Debug, PartialEq)]
pub struct AppliedOverlay {
    /// The volume whose overlap fired the overlay.
    pub trigger: WorldObjectId,
    /// The object the effect was applied to.
    pub target: WorldObjectId,
    /// The displacement that was applied, in canonical meters.
    pub offset_m: [f64; 3],
    /// Every entity the displacement was written to, in the object's own
    /// entity order: the render's and the collision's.
    pub entities: Vec<Entity>,
}

/// Why an overlay was not applied.
///
/// Every variant names what was asked for. A refused overlay is never recorded
/// as applied, so the same request may be made again — the refusal is a
/// statement about this attempt, not a permanent state.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum OverlayError {
    /// No world is loaded, so there is no load whose overlays these could be.
    NoResidentWorld,
    /// The resident load declares no overlay for this trigger. A sensor volume
    /// with no overlay behind it is a volume that reports an overlap, which is
    /// exactly what its authored role means, so a *contact* naming it is not a
    /// failure; a caller that names it directly gets this answer rather than
    /// silence.
    NoSuchOverlay {
        /// The volume the caller named.
        trigger: WorldObjectId,
    },
    /// The trigger's effect target is not present: its sector is streamed out
    /// right now. The overlay is still unapplied, so a later contact or an
    /// explicit request applies it.
    TargetNotPresent {
        /// The volume that was crossed.
        trigger: WorldObjectId,
        /// The object the effect names, which is not present.
        target: WorldObjectId,
    },
    /// This load has already applied the overlay fired by `trigger`. "Once" is
    /// the load's rule, not the frame's.
    AlreadyApplied {
        /// The volume that was crossed.
        trigger: WorldObjectId,
        /// The object the effect names, which is already displaced.
        target: WorldObjectId,
    },
    /// The target was present according to the load record, but one of its
    /// entities is gone. Nothing was moved, so the inconsistency stays visible
    /// instead of being papered over with a half-displaced object.
    VanishedEntity {
        /// The object the effect names.
        target: WorldObjectId,
        /// The entity that was gone.
        entity: Entity,
        /// What the world reported.
        reason: String,
    },
    /// The target is **sheared**: its presentation carries the whole authored
    /// affine in its `GlobalTransform` and deliberately no `Transform`, because
    /// a `Transform` beside it would make Bevy's propagation replace the shear
    /// with a translation/rotation/scale on the first update
    /// (`super::affine::pose_for`). There is therefore no component for a
    /// displacement to write to.
    ///
    /// Refused rather than half-applied: moving the collider's `Position`
    /// alone is exactly the mismatch this module exists to prevent — a drawn
    /// object that opens while its collision stays shut, which passes any test
    /// that reads the pose after a hundred ticks.
    Undisplaceable {
        /// The object the effect names.
        target: WorldObjectId,
        /// The entity with no `Transform` to move.
        entity: Entity,
    },
}

impl std::fmt::Display for OverlayError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::NoResidentWorld => {
                write!(f, "no world is loaded, so it declares no overlays")
            }
            Self::NoSuchOverlay { trigger } => write!(
                f,
                "the resident load declares no overlay for the trigger `{trigger}`"
            ),
            Self::TargetNotPresent { trigger, target } => write!(
                f,
                "the overlay triggered by `{trigger}` moves `{target}`, which is not \
                 present: its sector is streamed out"
            ),
            Self::AlreadyApplied { trigger, target } => write!(
                f,
                "the overlay triggered by `{trigger}` has already moved `{target}`"
            ),
            Self::VanishedEntity {
                target,
                entity,
                reason,
            } => write!(
                f,
                "the overlay moves `{target}`, but its entity {entity} is gone ({reason})"
            ),
            Self::Undisplaceable { target, entity } => write!(
                f,
                "the overlay moves `{target}`, but its entity {entity} is sheared and carries \
                 no `Transform` to move: writing only the collider's position would leave the \
                 drawn half behind"
            ),
        }
    }
}

impl std::error::Error for OverlayError {}

/// One thing the consumer pass did with an overlay, applied or not.
#[derive(Clone, Debug, PartialEq)]
pub enum OverlayOutcome {
    /// The overlay was applied.
    Applied(AppliedOverlay),
    /// The overlay was not, and why. The reason is the same value
    /// [`apply_overlay`] returned, so the trace and the caller cannot disagree.
    Refused {
        /// The trigger that was crossed, or named.
        trigger: WorldObjectId,
        /// The refusal.
        reason: OverlayError,
    },
}

impl OverlayOutcome {
    /// Whether this outcome applied its overlay.
    #[must_use]
    pub const fn is_applied(&self) -> bool {
        matches!(self, Self::Applied(_))
    }

    /// The application, when there is one.
    #[must_use]
    pub const fn applied(&self) -> Option<&AppliedOverlay> {
        match self {
            Self::Applied(applied) => Some(applied),
            Self::Refused { .. } => None,
        }
    }

    /// The refusal, when there is one.
    #[must_use]
    pub const fn refusal(&self) -> Option<&OverlayError> {
        match self {
            Self::Applied(_) => None,
            Self::Refused { reason, .. } => Some(reason),
        }
    }
}

/// The consumer pass's trace for the current session.
///
/// Append-only within a run, like [`super::contacts::WorldContacts`]: gameplay
/// and a test read it to see which authored object a body reached and what the
/// load did about it. It is a *trace*, not state: the authoritative record of
/// what a load applied is
/// [`ResidentWorld::applied_overlays`](super::residency::ResidentWorld::applied_overlays).
#[derive(Resource, Default, Debug)]
pub struct WorldOverlayLog {
    outcomes: Vec<OverlayOutcome>,
}

impl WorldOverlayLog {
    /// Every outcome, in the order the consumer pass produced them.
    #[must_use]
    pub fn outcomes(&self) -> &[OverlayOutcome] {
        &self.outcomes
    }

    /// The overlays that were applied, in order.
    #[must_use]
    pub fn applied(&self) -> Vec<&AppliedOverlay> {
        self.outcomes
            .iter()
            .filter_map(OverlayOutcome::applied)
            .collect()
    }

    /// The refusals, in order.
    #[must_use]
    pub fn refusals(&self) -> Vec<(&WorldObjectId, &OverlayError)> {
        self.outcomes
            .iter()
            .filter_map(|outcome| match outcome {
                OverlayOutcome::Refused { trigger, reason } => Some((trigger, reason)),
                OverlayOutcome::Applied(_) => None,
            })
            .collect()
    }

    /// Appends one outcome.
    pub fn record(&mut self, outcome: OverlayOutcome) {
        self.outcomes.push(outcome);
    }

    /// Drops every recorded outcome.
    pub fn clear(&mut self) {
        self.outcomes.clear();
    }
}

/// The loaded world's overlay record, or [`OverlayError::NoResidentWorld`].
fn resident(world: &bevy::prelude::World) -> Result<&ResidentWorld, OverlayError> {
    residency(world)
        .map(WorldResidency::resident)
        .ok_or(OverlayError::NoResidentWorld)
}

/// Applies the overlay the resident load declares for `trigger`.
///
/// This is the consumer's unit, and the one a test calls directly. It decides
/// everything before it changes anything: the load, the overlay, whether it has
/// already been applied, whether the target is present and whether every entity
/// the target owns is really there are all established first, so a refusal
/// leaves the door exactly where it was — and, crucially, leaves the overlay
/// *unapplied*, so the request can be made again.
///
/// # Errors
///
/// [`OverlayError::NoResidentWorld`] when nothing is loaded,
/// [`OverlayError::TargetNotPresent`] when the effect's target is streamed out,
/// [`OverlayError::AlreadyApplied`] when this load has already applied it, and
/// [`OverlayError::VanishedEntity`] when a presented object has lost an entity
/// somewhere else.
pub fn apply_overlay(
    world: &mut bevy::prelude::World,
    trigger: &WorldObjectId,
) -> Result<AppliedOverlay, OverlayError> {
    // 1. Decide, read-only.
    let resident = resident(world)?;
    let Some(overlay) = resident.overlay_for(trigger) else {
        return Err(OverlayError::NoSuchOverlay {
            trigger: trigger.clone(),
        });
    };
    let OverlayEffect::Displace { target, offset_m } = overlay.effect();
    let target = target.clone();
    let offset_m = *offset_m;
    if resident.is_applied(trigger) {
        return Err(OverlayError::AlreadyApplied {
            trigger: trigger.clone(),
            target: target.clone(),
        });
    }
    let Some(spawned) = resident.object(&target) else {
        return Err(OverlayError::TargetNotPresent {
            trigger: trigger.clone(),
            target: target.clone(),
        });
    };
    let spawned = spawned.clone();
    let entities = spawned.entities();
    for entity in &entities {
        world
            .get_entity(*entity)
            .map_err(|reason| OverlayError::VanishedEntity {
                target: target.clone(),
                entity: *entity,
                reason: format!("{reason:?}"),
            })?;
    }

    // 2. Change. The movement first, then the record, so a query and the
    //    resource cannot disagree about whether the door is open: a pass that
    //    reached the record and not the entities would report a door that never
    //    moved, and one that reached the entities and not the record would move
    //    it twice.
    displace_object(world, &spawned, offset_vec3(offset_m))?;
    if let Some(mut residency) = world.get_resource_mut::<WorldResidency>() {
        residency.resident_mut().mark_applied(trigger.clone());
    }
    Ok(AppliedOverlay {
        trigger: trigger.clone(),
        target,
        offset_m,
        entities,
    })
}

/// One canonical offset as the runtime's vector.
fn offset_vec3(offset_m: [f64; 3]) -> Vec3 {
    Vec3::new(offset_m[0] as f32, offset_m[1] as f32, offset_m[2] as f32)
}

/// Adds `offset` to every transform one object's entities carry.
///
/// The entity list is [`SpawnedObject::entities`] — the report's own — so the
/// displacement follows the entity layout: a cuboid object is a presentation
/// marker plus a collider entity, and a mesh object is the one entity that is
/// both. **Two** components are written per entity, and both are needed:
///
/// * [`Transform`], which is the render path's own transform and what a
///   `GlobalTransform` is computed from;
/// * Avian's [`Position`], which is what the narrow phase resolves a collider
///   against.
///
/// The measurement that makes this concrete, and the reason the two are not
/// "obviously" the same value: writing **only** `Transform` leaves the collider
/// where it was for the rest of the tick, and Avian's own
/// `PhysicsTransformPlugin::transform_to_position` copies it across at the start
/// of the *next* step. Measured on the depot panel: after a transform-only write
/// the entity's `Position` still read `z = 0`, and only after one further
/// `App::update` did it read `z = 2`. A door that visibly opens while its
/// collision is still shut for a tick is the mismatch F18 non-negotiable
/// behavior 1 exists to prevent, and it would pass any test that read the pose
/// after a hundred ticks.
///
/// Writing `GlobalTransform` as well is deliberately **not** done: Bevy's
/// `sync_simple_transforms` recomputes it from `Transform` at the start of the
/// next step anyway, so a second write would be a third value to keep in step
/// with no reader in between.
///
/// # Errors
///
/// [`OverlayError::VanishedEntity`] when one of the entities is gone. Checked
/// before anything is written, so a refusal moves nothing.
pub fn displace_object(
    world: &mut bevy::prelude::World,
    spawned: &SpawnedObject,
    offset: Vec3,
) -> Result<Vec<Entity>, OverlayError> {
    let entities = spawned.entities();
    for entity in &entities {
        world
            .get_entity(*entity)
            .map_err(|reason| OverlayError::VanishedEntity {
                target: spawned.object.clone(),
                entity: *entity,
                reason: format!("{reason:?}"),
            })?;
    }
    // Checked before anything is written, so a refusal moves nothing: an entity
    // with no `Transform` is a **sheared** object, and writing only its
    // collider's position is the render/collision split this module exists to
    // prevent. See [`OverlayError::Undisplaceable`].
    for entity in &entities {
        let pose_exists = world
            .get_entity(*entity)
            .map(|entity_ref| entity_ref.contains::<Transform>())
            .unwrap_or(false);
        if !pose_exists {
            return Err(OverlayError::Undisplaceable {
                target: spawned.object.clone(),
                entity: *entity,
            });
        }
    }
    for entity in &entities {
        let Ok(mut entity_ref) = world.get_entity_mut(*entity) else {
            // Unreachable: the loop above established every entity exists and
            // nothing in between despawns one.
            continue;
        };
        if let Some(mut transform) = entity_ref.get_mut::<Transform>() {
            transform.translation += offset;
        }
        if let Some(mut position) = entity_ref.get_mut::<Position>() {
            position.0 += offset;
        }
    }
    Ok(entities)
}

/// Re-applies this load's already-applied overlays to one freshly spawned
/// object.
///
/// Called by [`super::residency::load_sector`] for every object a sector load
/// spawns, beside the condition stamp: the load's memory is the condition *and*
/// the effect it has already applied, and a reloaded door that snapped shut
/// because its entities were rebuilt from the record would be a door the
/// player opened for nothing (F18 non-negotiable behavior 3, AC02's rule applied
/// to an overlay).
///
/// # Errors
///
/// [`OverlayError::VanishedEntity`] when the object lost an entity while its
/// overlays were being re-applied.
pub fn reapply_object(
    world: &mut bevy::prelude::World,
    spawned: &SpawnedObject,
) -> Result<(), OverlayError> {
    let Some(resident) = residency(world).map(WorldResidency::resident) else {
        return Ok(());
    };
    let applied: Vec<[f64; 3]> = resident
        .applied_overlays()
        .iter()
        .filter_map(|trigger| resident.overlay_for(trigger))
        .filter_map(|overlay| match overlay.effect() {
            OverlayEffect::Displace {
                target, offset_m, ..
            } if *target == spawned.object => Some(*offset_m),
            _ => None,
        })
        .collect();
    for offset_m in applied {
        displace_object(world, spawned, offset_vec3(offset_m))?;
    }
    Ok(())
}

/// The producer: records every sensor volume a body reached this tick.
///
/// The same [`CollisionStart`] stream [`super::contacts::record_world_contacts`]
/// reads, and the same "exactly one side is world geometry" rule: two world
/// objects touching is authoring, not gameplay. A contact whose world side is
/// not a sensor is not a trigger, so a solid object a body merely flew past is
/// never asked about.
///
/// The producer does not consult the load. It cannot — a system in the physics
/// schedule has no business reading a world record — and it does not need to:
/// a sensor volume the load declares no overlay for is a volume that reports an
/// overlap, which is what its role means, and the consumer answers that by
/// doing nothing.
///
/// # What the role filter is and is not pinned by
///
/// The `role != Sensor` check is here because a trigger's role is the record's
/// own claim about whether a body can *enter* it
/// ([`MissionOverlay`](cs_content::world::MissionOverlay) refuses any other), and
/// a producer that forwarded a solid contact would ask the consumer to apply an
/// overlay for a wall.
///
/// It is **not** independently observable through the public surface, and that
/// is worth saying rather than pretending otherwise: the consumer independently
/// requires the load to *declare* an overlay for whatever it is handed, and
/// `load_world` refuses at the door a load whose trigger is not a sensor
/// ([`WorldError::OverlayTriggerNotASensor`](cs_content::world::WorldError)),
/// so no reachable load can give the filter anything to be wrong about. A
/// mutation that deletes the check leaves every test in the suite green. The
/// filter is kept because it is the check the record's own contract states, and
/// because the consumer's filter is a different one — *does the load declare
/// this?* rather than *can a body enter this?* — but a reviewer should read it as
/// stated and not as covered.
pub fn queue_overlay_triggers(
    mut reader: MessageReader<CollisionStart>,
    colliders: Query<&WorldColliderInstance>,
    bindings: Query<&WorldObjectBinding>,
    mut requests: ResMut<OverlayTriggerRequests>,
) {
    for event in reader.read() {
        let world_first = colliders.get(event.collider1).is_ok();
        let world_second = colliders.get(event.collider2).is_ok();
        if world_first == world_second {
            continue;
        }
        let world_entity = if world_first {
            event.collider1
        } else {
            event.collider2
        };
        let Ok(marker) = colliders.get(world_entity) else {
            continue;
        };
        if marker.role() != WorldCollisionRole::Sensor {
            continue;
        }
        let Ok(binding) = bindings.get(world_entity) else {
            continue;
        };
        requests.request(binding.object().clone());
    }
}

/// The consumer: drains the hand-off and applies every overlay the resident load
/// declares for it.
///
/// An exclusive pass, because applying an effect moves entities and the record
/// that says they moved. Each request is decided and applied on its own, so one
/// refusal does not stop the next overlay: the trace is what says what happened,
/// and a consumer that dropped the rest of the queue on the first failure would
/// make a mission's remaining overlays depend on the order of its sensors.
///
/// A request the load has no overlay for is not a refusal and is not traced: a
/// sensor volume that reports an overlap is the record's own role, and a trace
/// that listed it would say a mission misconfigured itself every time a body
/// crossed an ordinary trigger.
pub fn apply_overlay_requests(world: &mut bevy::prelude::World) {
    let drained = world
        .get_resource_mut::<OverlayTriggerRequests>()
        .map_or_else(BTreeSet::new, |mut requests| requests.drain());
    if drained.is_empty() {
        return;
    }
    for trigger in drained {
        if residency(world)
            .map(WorldResidency::resident)
            .and_then(|resident| resident.overlay_for(&trigger))
            .is_none()
        {
            continue;
        }
        let outcome = match apply_overlay(world, &trigger) {
            Ok(applied) => OverlayOutcome::Applied(applied),
            Err(reason) => OverlayOutcome::Refused { trigger, reason },
        };
        if let Some(mut log) = world.get_resource_mut::<WorldOverlayLog>() {
            log.record(outcome);
        }
    }
}

/// Installs the overlay producer, the hand-off and the consumer pass.
///
/// Installed by the **app composition**
/// ([`super::fixture::world_app`]), not by a load: a world is loaded after
/// `App::finish` in a real mission, where `add_plugins` panics, and the same
/// reasoning as [`super::contacts::WorldPlugin`] applies. An app that runs a
/// world without this plugin never fires an overlay — a gap in that
/// composition, not something a load can repair.
pub struct WorldOverlayPlugin;

impl Plugin for WorldOverlayPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<OverlayTriggerRequests>()
            .init_resource::<WorldOverlayLog>()
            .add_systems(
                FixedPostUpdate,
                (
                    queue_overlay_triggers.after(PhysicsSystems::StepSimulation),
                    apply_overlay_requests.after(queue_overlay_triggers),
                ),
            );
    }
}

/// The recorded trace of the current session.
#[must_use]
pub fn overlay_log(world: &bevy::prelude::World) -> Option<&WorldOverlayLog> {
    world.get_resource::<WorldOverlayLog>()
}

/// Asks for the overlay `trigger` to be applied on the next consumer pass.
///
/// The programmatic producer for a trigger that is not a sensor overlap — a
/// mission script's own "the player has the key" branch, say. It goes through
/// the same hand-off and the same consumer as the contact stream, so there is
/// one place that applies an effect and one trace that says what it did.
///
/// # Panics
///
/// If the app has no [`OverlayTriggerRequests`] resource, which means
/// [`WorldOverlayPlugin`] was never added. A composition that runs a world
/// without the overlay pass has no consumer to hand the request to.
pub fn request_overlay(app: &mut App, trigger: WorldObjectId) {
    app.world_mut()
        .get_resource_mut::<OverlayTriggerRequests>()
        .expect("the overlay hand-off is installed by WorldOverlayPlugin")
        .request(trigger);
}
