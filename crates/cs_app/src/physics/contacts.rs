//! Contact reporting: classified, deduplicated collision reports (F23-B).
//!
//! Spec: `specs/F23-avian-integration-collision-and-fixed-step-authority.md`,
//! stage `### F23-B`. Shared contract: `docs/contracts/FLIGHT-PHYSICS.md`,
//! section "Collision and ballistic tests".
//!
//! Avian reports that two colliders started touching; this module turns that
//! engine fact into the game's own vocabulary:
//!
//! * every event is classified with `cs_sim::collision::classify_contact`
//!   using the [`BodyLayer`] each spawned body carries, so a sensor overlap is
//!   reported as [`ContactKind::SensorOverlap`] and never as a solid contact
//!   (spec non-negotiable behavior 2: *sensor overlap is not damage by
//!   itself*);
//! * a pair is reported **once per contact episode**: a second start for a
//!   pair that is already active is counted as suppressed instead of being
//!   recorded, which is the "apply damage once even if several collision
//!   features report the same hit" rule of the contract;
//! * an event whose entity carries no [`BodyLayer`] (a body spawned outside
//!   [`spawn_body`](crate::physics::spawn_body)) is counted as unclassified
//!   rather than silently guessed, and an event the declared matrix says
//!   cannot interact is counted as ignored, so a binding mistake shows up as
//!   a counter instead of a missing hit.
//!
//! Reading runs in `FixedPostUpdate` **after** `PhysicsSystems::StepSimulation`,
//! the same tick that wrote the events
//! (`CollisionEventSystems` sits in `PhysicsStepSystems::Finalize`).
//!
//! **Designed reporting, not original data.** Which contacts the original
//! game turned into damage, and how it deduplicated them, is **unknown** until
//! F23-D measures it; this is declared project behavior.

use std::collections::HashSet;

use avian3d::prelude::{CollisionEnd, CollisionStart, PhysicsSystems, Sensor};
use bevy::{
    ecs::schedule::IntoScheduleConfigs,
    prelude::{
        App, Entity, FixedPostUpdate, MessageReader, Plugin, Query, Res, ResMut, Resource, With,
    },
};
use cs_sim::collision::{CollisionLayer, ContactKind, ShapeClass, classify_contact};

use super::adapter::PhysicsTickLedger;
use super::body::BodyLayer;
use super::preflight;

/// One classified contact start.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ContactReport {
    /// The two colliders involved, in ascending entity order.
    pub bodies: [Entity; 2],
    /// The declared layer of each body, aligned with [`Self::bodies`].
    pub layers: [CollisionLayer; 2],
    /// The shared classification of the pair.
    pub kind: ContactKind,
    /// The fixed tick that reported it (0 when no tick ledger is installed).
    pub tick: u64,
}

impl ContactReport {
    /// Whether either side of this contact is on `layer`.
    pub fn involves(self, layer: CollisionLayer) -> bool {
        self.layers[0] == layer || self.layers[1] == layer
    }
}

/// The contact reports of a running physics world.
#[derive(Resource, Debug, Default)]
pub struct ContactReports {
    reports: Vec<ContactReport>,
    active: HashSet<[Entity; 2]>,
    tick: u64,
    total: u64,
    suppressed: u64,
    unclassified: u64,
    ignored: u64,
}

impl ContactReports {
    /// The reports of the current tick. Cleared when the tick advances.
    pub fn reports(&self) -> &[ContactReport] {
        &self.reports
    }

    /// How many contact pairs are considered active right now.
    ///
    /// A pair leaves the set on its `CollisionEnd` or when either side no
    /// longer carries a [`BodyLayer`] — Avian does not guarantee an end
    /// event for a despawned collider, so the reporter prunes a pair whose
    /// body is gone instead of retaining it until an end that never comes
    /// (F23-C retention rule; F23-B limitation 2).
    pub fn active_len(&self) -> usize {
        self.active.len()
    }

    /// Reports recorded since the resource was built or cleared.
    pub fn total(&self) -> u64 {
        self.total
    }

    /// Repeat starts of an already-active pair, suppressed as duplicates.
    pub fn suppressed(&self) -> u64 {
        self.suppressed
    }

    /// Events whose entity carried no [`BodyLayer`] marker.
    pub fn unclassified(&self) -> u64 {
        self.unclassified
    }

    /// Events the declared interaction matrix says cannot happen.
    pub fn ignored(&self) -> u64 {
        self.ignored
    }

    /// The tick the current report batch belongs to.
    pub fn tick(&self) -> u64 {
        self.tick
    }

    /// Drops every report and counter, including the active-pair set.
    pub fn clear(&mut self) {
        self.reports.clear();
        self.active.clear();
        self.tick = 0;
        self.total = 0;
        self.suppressed = 0;
        self.unclassified = 0;
        self.ignored = 0;
    }

    fn begin_tick(&mut self, tick: u64) {
        if tick != self.tick {
            self.tick = tick;
            self.reports.clear();
        }
    }

    fn end(&mut self, pair: [Entity; 2]) {
        self.active.remove(&pair);
    }

    fn record(
        &mut self,
        pair: [Entity; 2],
        layers: [CollisionLayer; 2],
        kind: ContactKind,
        tick: u64,
    ) {
        if kind == ContactKind::Ignored {
            self.ignored += 1;
            return;
        }
        if !self.active.insert(pair) {
            self.suppressed += 1;
            return;
        }
        self.total += 1;
        self.reports.push(ContactReport {
            bodies: pair,
            layers,
            kind,
            tick,
        });
    }
}

fn pair_of(a: Entity, b: Entity) -> [Entity; 2] {
    if a <= b { [a, b] } else { [b, a] }
}

/// Reads Avian's collision events and records classified contact reports.
fn record_contact_reports(
    mut starts: MessageReader<CollisionStart>,
    mut ends: MessageReader<CollisionEnd>,
    mut reports: ResMut<ContactReports>,
    layers: Query<&BodyLayer>,
    sensors: Query<(), With<Sensor>>,
    ledger: Option<Res<PhysicsTickLedger>>,
) {
    let tick = ledger.map_or(0, |ledger| ledger.ticks);
    reports.begin_tick(tick);

    // Despawn retention: a pair whose body is gone (or whose `BodyLayer` was
    // removed) leaves the active set — Avian does not promise a
    // `CollisionEnd` for a collider that despawned mid-contact.
    reports
        .active
        .retain(|pair| pair.iter().all(|entity| layers.contains(*entity)));

    for event in ends.read() {
        reports.end(pair_of(event.collider1, event.collider2));
    }

    for event in starts.read() {
        let (a, b) = (event.collider1, event.collider2);
        let (Ok(layer_a), Ok(layer_b)) = (layers.get(a), layers.get(b)) else {
            reports.unclassified += 1;
            continue;
        };
        let shape = |entity: Entity| {
            if sensors.contains(entity) {
                ShapeClass::Sensor
            } else {
                ShapeClass::Solid
            }
        };
        let kind = classify_contact(layer_a.0, layer_b.0, shape(a), shape(b));
        let pair = pair_of(a, b);
        let layers = if pair[0] == a {
            [layer_a.0, layer_b.0]
        } else {
            [layer_b.0, layer_a.0]
        };
        reports.record(pair, layers, kind, tick);
    }
}

/// The F23-B runtime: contact reporting on top of the F23-A adapter.
///
/// The plugin needs [`PhysicsTickLedger`] (installed by
/// [`PhysicsAdapterPlugin`](crate::physics::PhysicsAdapterPlugin)) only to
/// stamp reports with their tick; without it reports are still recorded with
/// tick 0.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct PhysicsBodiesPlugin;

impl Plugin for PhysicsBodiesPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<ContactReports>();
        app.add_systems(
            FixedPostUpdate,
            record_contact_reports.after(PhysicsSystems::StepSimulation),
        );
        // The spawn-side half of the bodies runtime: first-tick swept
        // preflight for the CCD layers (F23-C).
        preflight::install(app);
        // The resting rule (#428): a body whose contact residual never decays
        // comes to rest. It is a plugin of its own because the world
        // composition needs it too and does not install this one.
        app.add_plugins(super::resting::RestingBodiesPlugin);
    }
}
