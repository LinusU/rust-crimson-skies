//! Trigger crossings: the gameplay consumer for the swept spawn preflight's
//! sensor record (task #415).
//!
//! Spec: `specs/F39-objectives-triggers-timers-spawn-groups-and-dialogue-cues.md`
//! (F39 owns trigger semantics), stage `### F39-A` for the crossing
//! vocabulary this reuses, and
//! `specs/F23-avian-integration-collision-and-fixed-step-authority.md` for the
//! producer. Shared contracts: `docs/contracts/FLIGHT-PHYSICS.md`
//! ("Collision and ballistic tests": *"For interaction triggers use a swept
//! center/shape appropriate to the original rule"*) and
//! `docs/contracts/SCRIPT-MISSION.md` ("Objective event ordering").
//!
//! # The record, and the decision this module makes about it
//!
//! F23-D closed the **record** half of a measured engine hole and deliberately
//! left the **rule** half open. A body spawned inside one tick of travel of an
//! obstacle is not in the broad phase for its first tick (F23-B limitation 1),
//! and F23-C's criterion `accept_f23_c_preflight_never_stops_on_a_sensor`
//! forbids a sensor from stopping or delaying that spawn — so a fast body
//! leaves a thin trigger *inside* the very tick it is invisible to collision
//! detection, and the engine reports **no** overlap for it. Measured at
//! 120 Hz: a projectile spawned 0.4 ticks of travel short of a 2 cm trigger
//! crosses it and leaves at its fired speed with zero contact reports, at every
//! probed speed and rate.
//!
//! The second, non-blocking cast in [`SpawnPreflightEvent::passed`] records that
//! crossing, with the distance at which the body's own collider met the volume,
//! whether or not a solid obstacle stopped the same tick. F23-D wrote down that
//! *"whether gameplay consumes that field as a trigger crossing is a rule
//! F23-C's criterion does not decide"*. This module is that decision:
//!
//! > **The spawn-tick crossing is delivered as a gameplay crossing.** It is not
//! > a diagnostic. A trigger crossed inside the spawn tick reaches the
//! > authoritative event stream on the tick it happened, exactly once per
//! > `(actor, volume)` pair, and nothing about the spawn changes: the body
//! > keeps its velocity and a trigger still never stops or delays anything
//! > (F23-C's criterion is not weakened, and
//! > `accept_t415_a_the_delivery_does_not_move_or_delay_the_body` measures it).
//!
//! The alternative — declaring the field diagnostic-only — was rejected on the
//! affected content rather than on taste: the record's own consumers are
//! mission triggers, and a mission volume a body can cross and never report is
//! a mission that cannot complete. The full decision, the measurement behind it
//! and the boundaries of the claim are in
//! `docs/findings/2026-10-02-t415-spawn-tick-trigger-crossing.md`.
//!
//! # The crossing decision, made once
//!
//! The decision is not "the preflight said so". It is the same rule the
//! ordinary-flight path must use (task #498), stated so the two cannot drift:
//!
//! * a crossing is decided from the **body's own motion over the tick** — a
//!   segment swept against the volume, or the engine's own time of impact
//!   along that segment — never from a sampled overlap, which is precisely
//!   what a body faster than the volume is thick cannot produce;
//! * the segment is the one the body would have swept, so the decision does not
//!   depend on the tick rate: a body that covers the volume in one tick at
//!   120 Hz covers it in one tick at 60 Hz, and the crossing is reported in
//!   both;
//! * each `(actor, volume)` pair enters the stream **once**: a body that stays
//!   inside a volume, or a record a multi-tick frame re-reads, produces no
//!   second entry. `duplicates` counts the ones that were refused rather than
//!   letting a second entry through;
//! * delivery is a *read*. [`deliver_spawn_tick_crossings`] takes the preflight
//!   log and this module's own resource and nothing else — no `Query`, no
//!   `Commands`, no mutable access to a pose or a velocity — so it cannot stop
//!   or delay a body even by accident, and a trigger is never an obstacle.
//!
//! # What this module does not do
//!
//! * **It does not name content.** A crossing names the volume's `Entity`; the
//!   `cs_script::ir::SymbolId` a mission program uses, and the
//!   `cs_sim::objectives::SweptTrigger` that pairs a symbol with an `ActorId`
//!   and a `Volume`, are F39-B's content binding (already named as deferred in
//!   `docs/findings/2026-10-01-f39-a-objective-trigger-spawn-semantics.md`).
//!   Inventing a symbol for a collider would be a guess about content, and a
//!   guessed content id in a mission's trigger table is worse than a missing
//!   one.
//! * **It does not apply an effect.** F18-C's
//!   [`OverlayTriggerRequests`](crate::world::OverlayTriggerRequests) is where
//!   a crossed volume opens a door; that hand-off is fed by the engine's
//!   `CollisionStart` stream, which reports nothing for a spawn-tick crossing,
//!   and the world-authored trigger volumes the mission would bind are not yet
//!   visible to the preflight's cast (they carry no
//!   [`BodyLayer`](crate::physics::BodyLayer)). Both gaps are composition
//!   defects in F18's and F23-C's paths, recorded in the finding and filed
//!   separately; wiring this channel into the overlay hand-off before they are
//!   closed would be an unreachable branch.
//! * **It claims nothing about the original game.** Whether the original
//!   reported a spawn-tick crossing at all, and with what delay, is **unknown**
//!   and is left to the calibration stage (F26). What is measured here is this
//!   project on the pinned pair (`bevy 0.19.1` / `avian3d 0.7.0`).

use std::collections::HashSet;

use avian3d::prelude::PhysicsSystems;
use bevy::{
    ecs::schedule::IntoScheduleConfigs,
    prelude::{App, Entity, FixedPostUpdate, Plugin, Res, ResMut, Resource},
};
use cs_sim::objectives::trigger::CrossingKind;

use crate::physics::{PhysicsTickLedger, SpawnPreflightLog};

/// Which producer decided a crossing.
///
/// A crossing carries its source so a consumer can tell a *swept* decision
/// (this one) from a sampled overlap, and so a second producer can be added
/// later — the ordinary-flight path of task #498 — without a crossing from one
/// of them being indistinguishable from the other's.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum CrossingSource {
    /// The swept spawn preflight's non-blocking sensor cast
    /// ([`SpawnPreflightEvent::passed`](crate::physics::SpawnPreflightEvent::passed)):
    /// the crossing a body made inside the tick it was spawned in and is
    /// therefore invisible to collision detection.
    SpawnTickPreflight,
}

impl std::fmt::Display for CrossingSource {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::SpawnTickPreflight => f.write_str("spawn_tick_preflight"),
        }
    }
}

/// One trigger crossing, delivered to gameplay.
///
/// The `kind` is the crossing's own direction; the pair enters the stream once
/// (see [`TriggerCrossings::record`]), so a consumer may treat an entry as an
/// event and never as a level.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct TriggerCrossing {
    /// The swept body that crossed the volume.
    pub actor: Entity,
    /// The sensor volume it crossed.
    pub volume: Entity,
    /// The fixed tick the crossing was decided on, from the adapter's ledger.
    pub tick: u64,
    /// Whether this is the entry or the exit.
    pub kind: CrossingKind,
    /// Which producer decided it.
    pub source: CrossingSource,
    /// How far along the spawn tick's travel the body met the volume, in
    /// meters, measured from the position it spawned at. The same reference the
    /// clamp's distance uses, so a consumer can order a crossing against a
    /// solid stop on the same tick.
    pub distance_m: f32,
}

impl TriggerCrossing {
    /// Whether this is an entry.
    #[must_use]
    pub const fn is_entry(&self) -> bool {
        matches!(self.kind, CrossingKind::Entry)
    }

    /// Whether this is an exit.
    #[must_use]
    pub const fn is_exit(&self) -> bool {
        matches!(self.kind, CrossingKind::Exit)
    }

    /// Whether `actor` is the body that crossed.
    #[must_use]
    pub fn is_by(&self, actor: Entity) -> bool {
        self.actor == actor
    }

    /// Whether `volume` is the volume that was crossed.
    #[must_use]
    pub fn is_of(&self, volume: Entity) -> bool {
        self.volume == volume
    }
}

/// The crossings delivered to gameplay, in the order they were decided.
///
/// A drain-what-you-read buffer in the shape of
/// [`SpawnPreflightLog`](crate::physics::SpawnPreflightLog) and
/// [`ContactReports`](crate::physics::ContactReports): a crossing is appended
/// once and a consumer takes the whole batch, so a crossing can never fire
/// twice or be silently dropped. It reads the preflight log **without**
/// draining it, so the session's own per-frame hand-off of the same records
/// ([`SessionFrame::spawn_events`](crate::physics::SessionFrame::spawn_events))
/// is unaffected.
#[derive(Resource, Debug, Default)]
pub struct TriggerCrossings {
    crossings: Vec<TriggerCrossing>,
    delivered: u64,
    duplicates: u64,
    /// The `(actor, volume)` pairs already in the stream.
    pairs: HashSet<[Entity; 2]>,
}

impl TriggerCrossings {
    /// The undelivered-to-consumer crossings, in decision order.
    #[must_use]
    pub fn crossings(&self) -> &[TriggerCrossing] {
        &self.crossings
    }

    /// The crossings of one actor, in decision order.
    #[must_use]
    pub fn crossings_by(&self, actor: Entity) -> Vec<&TriggerCrossing> {
        self.crossings
            .iter()
            .filter(|crossing| crossing.is_by(actor))
            .collect()
    }

    /// Crossings delivered since the resource was built or cleared.
    #[must_use]
    pub fn delivered(&self) -> u64 {
        self.delivered
    }

    /// Records refused because the pair was already in the stream.
    ///
    /// A record is re-read on every tick between the one that produced it and
    /// the one that drains it — a multi-tick render frame reads the same
    /// preflight record on each of its ticks — so this counter is what makes
    /// "exactly once" a property of the consumer rather than of the frame
    /// length.
    #[must_use]
    pub fn duplicates(&self) -> u64 {
        self.duplicates
    }

    /// Takes every recorded crossing, leaving the counters and the ledger.
    pub fn take(&mut self) -> Vec<TriggerCrossing> {
        core::mem::take(&mut self.crossings)
    }

    /// Drops the crossings and the counters, keeping the pair ledger.
    ///
    /// A consumer that has taken its batch does not want the same crossing
    /// again, so taking and clearing are different acts: only this one is for a
    /// world that is being reset.
    pub fn clear(&mut self) {
        self.crossings.clear();
        self.delivered = 0;
        self.duplicates = 0;
    }

    /// Drops the crossings, the counters and the pair ledger.
    ///
    /// The ledger is per world, and a body can preflight only once in a
    /// world's life, so this belongs to a teardown rather than to a consumer
    /// pass: keeping it across a reset would refuse every crossing of an entity
    /// id the new world reused.
    pub fn reset(&mut self) {
        self.clear();
        self.pairs.clear();
    }

    /// The one place a crossing enters the stream.
    ///
    /// Returns whether it was delivered. A `(actor, volume)` pair that is
    /// already in the stream is a duplicate: it is counted and refused, so a
    /// crossing fires exactly once however many times its record is read.
    pub fn record(&mut self, crossing: TriggerCrossing) -> bool {
        if !self.pairs.insert([crossing.actor, crossing.volume]) {
            self.duplicates += 1;
            return false;
        }
        self.delivered += 1;
        self.crossings.push(crossing);
        true
    }
}

/// Turns the swept spawn preflight's sensor record into gameplay crossings.
///
/// Runs in `FixedPostUpdate` after `PhysicsSystems::StepSimulation`, which is
/// after the preflight itself (it is scheduled before
/// `PhysicsSystems::Prepare`), so a record is delivered on the tick that
/// produced it — a projectile that crossed a trigger and was then stopped by a
/// wall in the same tick gets both facts on that tick, in the tick's order.
///
/// It is a pure read of two resources. That is the whole non-blocking claim:
/// the system holds no `Query`, issues no `Commands` and takes no mutable
/// access to a pose or a velocity, so a delivery cannot stop a body, delay it
/// or move it. A trigger never stops or delays anything (F23-C), and the
/// preflight's own second cast is non-blocking for the same reason.
///
/// A record that names a sensor without the distance the preflight measured it
/// at is refused rather than turned into a crossing with an invented geometry.
/// The two fields come from one `Option` in the producer, so a record with
/// only one of them is a producer defect, not a gameplay case.
fn deliver_spawn_tick_crossings(
    preflight: Res<SpawnPreflightLog>,
    ledger: Option<Res<PhysicsTickLedger>>,
    mut crossings: ResMut<TriggerCrossings>,
) {
    let tick = ledger.map_or(0, |ledger| ledger.ticks);
    for event in preflight.events() {
        let (Some(volume), Some(distance_m)) = (event.passed, event.passed_distance_m) else {
            continue;
        };
        crossings.record(TriggerCrossing {
            actor: event.body,
            volume,
            tick,
            // A body that was outside its volume and reached it inside one tick
            // entered it. A brand-new body's state is outside by construction:
            // the preflight cast is the decision, and there is no prior
            // observation for a turn-away or a return to suppress.
            kind: CrossingKind::Entry,
            source: CrossingSource::SpawnTickPreflight,
            distance_m,
        });
    }
}

/// Installs the spawn-tick trigger-crossing consumer.
///
/// Add it to the world that runs the preflight — the production
/// [`PhysicsSession`](crate::physics::PhysicsSession), whose
/// [`configure`](crate::physics::PhysicsSessionBuilder::configure) seam is the
/// place an app composes one. Without it the preflight still records the
/// crossing and gameplay still sees nothing, which is the gap this task closed.
///
/// The plugin needs [`PhysicsTickLedger`](crate::physics::PhysicsTickLedger) to
/// stamp a crossing with its tick and uses `0` when it is absent, like the
/// contact reporter does: an undated crossing is still a crossing, and a
/// missing ledger is not a reason to drop one.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct SpawnTickTriggerPlugin;

impl Plugin for SpawnTickTriggerPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<TriggerCrossings>().add_systems(
            FixedPostUpdate,
            deliver_spawn_tick_crossings.after(PhysicsSystems::StepSimulation),
        );
    }
}
