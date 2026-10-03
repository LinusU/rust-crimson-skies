//! Ordinary-flight swept trigger crossings (task #498).
//!
//! Spec: `specs/F18-world-geometry-terrain-water-and-traversable-interiors.md`,
//! stage `### F18-C` (the overlay a crossing feeds), coordinated with
//! `specs/F39-objectives-triggers-timers-spawn-groups-and-dialogue-cues.md`
//! (which owns trigger semantics: the [`TriggerCrossing`] record, the
//! [`TriggerCrossings`] stream and its once-per-pair ledger).
//! Shared contract: `docs/contracts/FLIGHT-PHYSICS.md` ("Collision and
//! ballistic tests": *"For interaction triggers use a swept center/shape
//! appropriate to the original rule"*).
//!
//! # The defect this producer closes
//!
//! Task #401 measured that a world trigger volume's report is a **discrete**
//! overlap: `queue_overlay_triggers` reads `CollisionStart`, which the narrow
//! phase emits only when a sample lands inside the volume. A body whose tick
//! outruns the volume's thickness — 3.33 m at 400 m/s over the depot's 1 m
//! cuboid trigger — produces no such sample, so the crossing and every overlay
//! behind it silently never fire; a mesh-derived volume has a second gap on
//! top, going quiet when a body lands deep inside with no triangle within
//! `max_contact_distance`. The full measurement is in
//! `docs/findings/2026-10-02-t401-trigger-volume-and-swept-ccd.md` and this
//! producer's own numbers are in
//! `docs/findings/2026-10-03-t498-swept-trigger-crossing.md`.
//!
//! # The rule, shared with the spawn tick
//!
//! Task #415 decided the crossing rule once
//! (`docs/findings/2026-10-02-t415-spawn-tick-trigger-crossing.md`) so the two
//! entry points cannot drift; this system is the ordinary-flight half of it:
//!
//! * a crossing is decided from the **body's own motion over the tick** — the
//!   segment from the `Position` the body ended the previous tick at to the one
//!   it ended this tick at, swept against the volume's own collider — never
//!   from a sampled overlap;
//! * the decision is rate-independent, because it is the segment the body
//!   covered and not how often a clock sampled it;
//! * each `(actor, volume)` pair enters the stream **once**, enforced by the
//!   one place a crossing enters it, [`TriggerCrossings::record`], so a body
//!   that dwells, turns away inside, or crosses and returns cannot fire twice;
//! * delivery is a **read**: the system takes `Position`, `Rotation`,
//!   `Collider` and the spatial query and nothing mutable on a body, so a
//!   trigger never stops, delays or nudges what it reports.
//!
//! # The inside bit
//!
//! What the spawn tick could not need and this path cannot skip is the
//! stateful per-pair "inside" bit — the same one
//! `cs_sim::objectives::SweptTrigger` keeps — because ordinary flight has the
//! transitions the spawn tick does not:
//!
//! * `outside → inside`: an **entry**, delivered;
//! * `outside → outside` with the segment touching the volume: a pass-through —
//!   an entry and an exit inside one tick, the entry delivered;
//! * `inside → outside`: an **exit** — tracked, and deliberately *not* a second
//!   stream record: the pair ledger is once-per-pair, and a body that crossed
//!   and returned is the same pair (rule 3), so an exit is bookkeeping the
//!   inside bit needs, not another crossing;
//! * `inside → inside`: nothing — a body that turns away inside a volume fires
//!   the report nowhere except on the entry it already made.
//!
//! The bit lives per **body**, not per pair: a body is not told which volumes
//! it may meet, so [`SweptBodyTracks`] stores the volumes its collider ended
//! the previous tick overlapping, which *is* the per-pair bit once the pair is
//! named.
//!
//! # What the decision is and is not made of
//!
//! The swept decision itself is two parry queries through Avian's
//! [`SpatialQuery`], run in `FixedPostUpdate` after
//! `PhysicsSystems::StepSimulation` so both segment endpoints are integrated
//! poses:
//!
//! * `shape_hits_callback` — the body's own [`Collider`] cast along the tick's
//!   segment — decides *touched*: which `WorldCollisionRole::Sensor` colliders
//!   the sweep crosses, on either layer arrangement (cuboid or mesh-derived),
//!   at any depth, because the cast asks the volume's own geometry and not a
//!   contact manifold;
//! * `shape_intersections_callback` at the segment's end decides *inside*:
//!   which sensor volumes the body ends the tick overlapping.
//!
//! The `inside` test shares the measured trimesh boundary from #401: a body
//! strictly inside a mesh volume overlaps no triangle, so parry's
//! `intersection_test` reports it only while a face is penetrated. That
//! narrows the *bit*, not the delivered stream: an entry is also fired on a
//! touched pass-through, the once-per-pair ledger contains the flicker, and an
//! exit is never a record. The boundary is named in the finding.
//!
//! # The overlay hand-off
//!
//! A delivered entry requests the volume's authored object's overlay through
//! the same [`OverlayTriggerRequests`] the `CollisionStart` producer writes —
//! one hand-off, one consumer, one trace. Whether the load declares an overlay
//! for it is the consumer's question, as it is for
//! [`super::overlays::queue_overlay_triggers`]; a sensor volume that reports an
//! overlap and opens nothing is exactly what its role means.
//!
//! # What is not claimed
//!
//! * **The first observed tick has no previous sample.** A body the pass sees
//!   for the first time is registered at its current pose and overlap set, and
//!   its motion into that pose is unobserved by construction — the spawn-tick
//!   entry is #415's producer, and a body teleported between samples reads as
//!   a long sweep (a direct `Position` write, an origin shift). Both edges are
//!   named in the finding rather than invented away.
//! * **A kinematic body is not swept.** `RigidBody::Kinematic` moves by
//!   prescription, not integration; it is a one-word extension if gameplay
//!   ever needs it.
//! * **Rotation is sampled at the end pose.** A body that pivots hard inside a
//!   tick sweeps its current orientation along the whole segment; the probes
//!   this stage measures fly straight.
//! * **Nothing about the original.** What the original's own trigger volumes
//!   measured, which of them a mission used, and whether it reported
//!   crossings this way are all unmeasured — `super::triggers` is the survey
//!   and #427 is its task.

use std::collections::hash_map::Entry;
use std::collections::{HashMap, HashSet};

use avian3d::prelude::{
    Collider, CollisionLayers as AvianCollisionLayers, PhysicsSystems, Position, RigidBody,
    Rotation, ShapeCastConfig, SpatialQuery, SpatialQueryFilter,
};
use bevy::ecs::schedule::IntoScheduleConfigs;
use bevy::prelude::{
    App, Dir3, Entity, FixedPostUpdate, Plugin, Quat, Query, Res, ResMut, Resource, Vec3,
};
use cs_content::world::WorldCollisionRole;
use cs_sim::collision::CollisionLayer;
use cs_sim::objectives::trigger::CrossingKind;

use crate::objectives::{CrossingSource, TriggerCrossing, TriggerCrossings};
use crate::physics::PhysicsTickLedger;

use super::contacts::{WorldColliderInstance, WorldObjectBinding};
use super::overlays::{OverlayTriggerRequests, apply_overlay_requests};

/// One moving body's crossing state: where it ended the previous tick and
/// which sensor volumes its collider ended that tick overlapping.
///
/// The `inside` set is the per-pair "inside" bit
/// `cs_sim::objectives::SweptTrigger` keeps, stored on the body because the
/// pairs are not declared anywhere: the set of volumes a body is inside, read
/// at each tick's end, is every `(body, volume)` inside-bit at once.
#[derive(Debug)]
struct BodyTrack {
    /// The body's `Position` at the end of the previous observed tick.
    position: Vec3,
    /// The sensor-volume entities the body's collider overlapped at the end of
    /// that tick.
    inside: HashSet<Entity>,
}

/// The swept crossing pass's tracking state and counters.
///
/// The state is what makes "exactly once per pair" and "no report for a body
/// that turns away inside" decidable at all; the counters are the producer's
/// own measurable record — how many casts ran, how many volume touches and
/// transitions they produced — so a finding can quote the pass rather than a
/// test's proxy for it.
#[derive(Resource, Debug, Default)]
pub struct SweptBodyTracks {
    bodies: HashMap<Entity, BodyTrack>,
    registered: u64,
    sweeps: u64,
    touches: u64,
    entries: u64,
    exits: u64,
}

impl SweptBodyTracks {
    /// Bodies the pass has registered and not yet pruned.
    #[must_use]
    pub fn tracked(&self) -> usize {
        self.bodies.len()
    }

    /// Bodies registered on first sight.
    #[must_use]
    pub fn registered(&self) -> u64 {
        self.registered
    }

    /// Segment casts run: one per tracked body per tick that moved.
    #[must_use]
    pub fn sweeps(&self) -> u64 {
        self.sweeps
    }

    /// Volume touches found by those casts, counted per `(body, volume)`.
    #[must_use]
    pub fn touches(&self) -> u64 {
        self.touches
    }

    /// Entry transitions delivered into [`TriggerCrossings`].
    ///
    /// A refused second record is not counted here: it is
    /// [`TriggerCrossings::duplicates`], which is where once-per-pair is
    /// enforced.
    #[must_use]
    pub fn entries(&self) -> u64 {
        self.entries
    }

    /// Exit transitions observed: a body leaving a volume it was inside, or a
    /// pass-through's exit half. Exits are bookkeeping for the inside bit —
    /// the stream is once-per-pair, so they are never second records.
    #[must_use]
    pub fn exits(&self) -> u64 {
        self.exits
    }
}

/// The volumes the body's collider overlaps at `position`, filtered to the
/// authored sensor role.
///
/// The mask already narrows the candidates to world-member colliders; the role
/// check is the record's own claim about which of them a body can *enter* —
/// the same check [`super::overlays::queue_overlay_triggers`] makes on the
/// contact stream, so the two producers cannot disagree about what a trigger
/// volume is.
fn sensor_overlaps(
    spatial: &SpatialQuery,
    collider: &Collider,
    position: Vec3,
    rotation: Quat,
    filter: &SpatialQueryFilter,
    volumes: &Query<&WorldColliderInstance>,
) -> HashSet<Entity> {
    let mut overlaps = HashSet::new();
    spatial.shape_intersections_callback(collider, position, rotation, filter, |entity| {
        if volumes
            .get(entity)
            .is_ok_and(|marker| marker.role() == WorldCollisionRole::Sensor)
        {
            overlaps.insert(entity);
        }
        true
    });
    overlaps
}

/// The ordinary-flight swept crossing producer.
///
/// Runs in `FixedPostUpdate` after `PhysicsSystems::StepSimulation`, where
/// every `Position` is the integrated end-of-tick pose, and before
/// [`apply_overlay_requests`], so a crossing decided on this tick applies its
/// overlay on this tick — the same latency the `CollisionStart` producer gives
/// the discrete path.
///
/// For each dynamic body it sweeps the tick's own segment (the previous
/// end-of-tick `Position` to this one) against the sensor volumes, decides the
/// transitions from the segment, the end overlap and the stored inside bit,
/// records each entry once into [`TriggerCrossings`], and requests the
/// overlay of every volume an entry was delivered for.
///
/// It is a **read** of the body: `Position`, `Rotation`, `Collider`,
/// `RigidBody` and `CollisionLayers` come in shared, and the only mutable
/// state is the pass's own tracking resource, the crossing stream and the
/// overlay hand-off. There is no `Query<&mut _>` and no `Commands`, so a
/// crossing cannot stop, delay or nudge the body it reports (rule 4).
#[allow(clippy::too_many_arguments)]
pub fn sweep_volume_crossings(
    spatial: SpatialQuery,
    tick: Option<Res<PhysicsTickLedger>>,
    bodies: Query<(
        Entity,
        &Collider,
        &Position,
        &Rotation,
        &RigidBody,
        &AvianCollisionLayers,
    )>,
    volumes: Query<&WorldColliderInstance>,
    bindings: Query<&WorldObjectBinding>,
    tracks: ResMut<SweptBodyTracks>,
    mut crossings: ResMut<TriggerCrossings>,
    mut requests: ResMut<OverlayTriggerRequests>,
) {
    let tick = tick.map_or(0, |ledger| ledger.ticks);
    // The world membership bit: world colliders — solid and sensor — all carry
    // `StaticWorld`, so a body's own filter set ANDed with it is exactly the
    // colliders the designed matrix lets that body meet. Symmetry of
    // `designed_collides_with` means this one bit covers both directions.
    let world_bit = u32::from(CollisionLayer::StaticWorld.bit());
    // Split the resource into its fields once: a `ResMut` derefs as a whole,
    // and the body's own track has to stay borrowed while the counters move.
    let tracks = tracks.into_inner();
    let SweptBodyTracks {
        bodies: body_tracks,
        registered,
        sweeps,
        touches,
        entries,
        exits,
    } = tracks;

    let mut seen = HashSet::new();
    for (entity, collider, position, rotation, rigid_body, layers) in bodies.iter() {
        if !rigid_body.is_dynamic() {
            continue;
        }
        seen.insert(entity);
        let mask = layers.filters.0 & world_bit;
        if mask == 0 {
            // A body that cannot interact with world geometry can cross no
            // world volume; the designed matrix already says so.
            continue;
        }
        let filter = SpatialQueryFilter::from_mask(mask).with_excluded_entities([entity]);
        let ends = sensor_overlaps(
            &spatial, collider, position.0, rotation.0, &filter, &volumes,
        );

        let track = match body_tracks.entry(entity) {
            // First sight: no previous sample exists, so there is no segment
            // to sweep. Register the pose and the overlap set — the inside bit
            // has to start somewhere, and "already inside" is the honest
            // answer for a body the pass first meets in a volume — and cast
            // nothing. A body that materialized inside a volume and leaves it
            // is an exit, not an invented entry.
            Entry::Vacant(vacant) => {
                vacant.insert(BodyTrack {
                    position: position.0,
                    inside: ends,
                });
                *registered += 1;
                continue;
            }
            Entry::Occupied(occupied) => occupied.into_mut(),
        };

        let segment = position.0 - track.position;
        let travel = segment.length();
        // The volumes the segment touched, each with the distance along the
        // segment at which the body's collider first met it.
        let mut touched = HashMap::new();
        if let Ok(direction) = Dir3::new(segment) {
            *sweeps += 1;
            let config = ShapeCastConfig::from_max_distance(travel);
            spatial.shape_hits_callback(
                collider,
                track.position,
                rotation.0,
                direction,
                &config,
                &filter,
                |hit| {
                    if volumes
                        .get(hit.entity)
                        .is_ok_and(|marker| marker.role() == WorldCollisionRole::Sensor)
                    {
                        touched.insert(hit.entity, hit.distance);
                    }
                    // Keep going: the segment is the whole tick, and a body can
                    // touch two volumes in one.
                    true
                },
            );
            *touches += touched.len() as u64;
        }

        // Every volume the pair could have changed state about: the ones the
        // body was inside, the ones it ends inside, and the ones the segment
        // touched. Sorted so a tick's decisions are in entity order, the way
        // the stream's "in decision order" reads best.
        let mut involved: Vec<Entity> = track
            .inside
            .union(&ends)
            .chain(touched.keys())
            .copied()
            .collect();
        involved.sort_unstable();
        for volume in involved {
            let was_inside = track.inside.contains(&volume);
            let ends_inside = ends.contains(&volume);
            let distance = match (was_inside, ends_inside) {
                // An entry: the body's segment crossed the boundary and it is
                // inside now, or — a teleport or a volume moved onto it — it
                // simply is inside now and was not. `SweptTrigger`'s
                // `(inside, ends_inside)` table makes the same two an entry.
                // `distance` is where along the tick's travel the collider met
                // the volume — the same frame of reference the spawn-tick
                // record uses — or the segment's whole length when there was
                // no boundary to meet (a jump, or a stationary body a moving
                // volume reached).
                (false, true) => Some(touched.get(&volume).copied().unwrap_or(travel)),
                // A pass-through: the boundary was crossed and the body is
                // outside again at the segment's end — entry and exit inside
                // one tick. The exit half is the inside bit's; only the entry
                // is a record.
                (false, false) => touched.get(&volume).copied(),
                // The exit half — the inside bit going off. Tracked, and
                // deliberately not a second record: the stream is
                // once-per-pair, and a body that crosses and returns is the
                // same pair.
                (true, false) => {
                    *exits += 1;
                    None
                }
                (true, true) => None,
            };
            let Some(distance_m) = distance else {
                continue;
            };
            if crossings.record(TriggerCrossing {
                actor: entity,
                volume,
                tick,
                kind: CrossingKind::Entry,
                source: CrossingSource::OrdinaryFlightSweep,
                distance_m,
            }) {
                *entries += 1;
                if let Ok(binding) = bindings.get(volume) {
                    requests.request(binding.object().clone());
                }
            }
            if !ends_inside {
                *exits += 1;
            }
        }
        track.position = position.0;
        track.inside = ends;
    }
    body_tracks.retain(|entity, _| seen.contains(entity));
}

/// Installs the ordinary-flight swept crossing producer.
///
/// Installed by the app composition ([`super::fixture::world_app`]), beside
/// the contact recorder and the overlay pass, for the same reason: a world is
/// loaded after `App::finish`, where `add_plugins` panics, and an app that
/// runs a world without this pass reports only the crossings the discrete
/// narrow phase can see — the measured hole this exists to close.
///
/// The plugin needs [`PhysicsTickLedger`] to stamp a crossing with its tick
/// and uses `0` when it is absent, as the spawn-tick consumer does: an undated
/// crossing is still a crossing, and a missing ledger is not a reason to drop
/// one.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct WorldSweptCrossingPlugin;

impl Plugin for WorldSweptCrossingPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<SweptBodyTracks>()
            .init_resource::<TriggerCrossings>()
            .add_systems(
                FixedPostUpdate,
                sweep_volume_crossings
                    .after(PhysicsSystems::StepSimulation)
                    .before(apply_overlay_requests),
            );
    }
}
