//! Remote aircraft state: the client-side snapshot ingest path (F57-A).
//!
//! Spec: `specs/F57-networked-aircraft-prediction-interpolation-and-projectiles.md`,
//! stage `### F57-A`. Contracts: `docs/contracts/UI-NETWORK.md`
//! ("Interpolation buffers separate actor generations"; "Motion snapshots are
//! sequenced and may be dropped"; reliable events are idempotent) and
//! `docs/contracts/FLIGHT-PHYSICS.md` (canonical space, one pose owner, a rebase
//! is a conversion and not an impulse).
//!
//! # What this stage owns, and what it deliberately does not
//!
//! [`cs_net::snapshot`] owns the wire schema, the quantizers and the declared
//! error budgets; [`cs_sim::net_state`] owns the server's authority and the
//! once-per-generation destruction gate. This module is the *receiver side
//! boundary* between them: it turns one decoded, server-authored
//! [`cs_net::snapshot::Snapshot`] plus the session's live world-origin epoch into
//! the local mirror of remote aircraft that the rest of the app reads.
//!
//! It is deliberately **not** the interpolation buffer and **not** local
//! prediction. Those are F57-B and build on what is established here: a
//! per-actor record that already knows its own generation, a mirror that refuses
//! to cross generations, and a retirement path that a lost sequenced snapshot
//! cannot undo. What is fixed now is the part both later stages depend on being
//! wrong-proof:
//!
//! * **Shared epoch.** Positions in a snapshot are quantized relative to an
//!   origin epoch. The ingest path refuses any snapshot whose epoch is not the
//!   live [`WorldOrigin`] epoch and reconstructs canonical world positions
//!   through [`WorldOrigin::world_of`] — the same conversion the local bodies
//!   use, applied exactly once. A snapshot from another epoch is therefore
//!   refused, never blended into a wrong world position (F57 AC03: "a rebase is
//!   not a huge velocity impulse").
//! * **Generations never mix.** A record whose generation differs from the one
//!   the mirror holds retires the old record and starts a new one, so no field
//!   of one generation is carried into another. That is what makes it safe for
//!   F57-B to key its interpolation history on `(actor, generation)` (F57 AC04).
//! * **Loss is not evidence.** An actor missing from one snapshot is *not*
//!   despawned: sequenced snapshots are droppable, so absence proves nothing.
//!   Retirement comes from the record's own lifecycle flag or from a reliable
//!   [`cs_net::message::EventBody::ActorRemoved`], which is why a session with
//!   10 % loss still ends with no ghost aircraft (F57 AC01).
//! * **Reordering is not rollback.** A snapshot older than the one already
//!   applied is refused whole. A late packet never rewinds a remote aircraft and
//!   never resurrects a generation that has already ended.
//! * **Presentation only.** Nothing here awards damage, consumes ammunition or
//!   resolves a hit. Those are server-owned (UI-NETWORK ownership table); the
//!   mirror only reports what the server said.
//!
//! All of this is newly authored engine design: no original networked behavior,
//! interpolation delay or remote-aircraft presentation has been measured, and
//! none is asserted here.

use std::collections::{BTreeMap, BTreeSet};
use std::fmt;

use cs_net::message::{EventBody, ReliableEvent};
use cs_net::snapshot::{
    ANGULAR_VELOCITY_QUANTIZATION, ActorRecord, Bank, ControlMode, DamageChannel, FlightChannel,
    LINEAR_VELOCITY_QUANTIZATION, Lifecycle, OriginEpoch, POSITION_QUANTIZATION, QuantizedRotation,
    QuantizedVector, Snapshot, SnapshotError, WeaponChannel,
};
use cs_types::Tick;
use cs_types::net::{ActorId, SessionId};
use cs_types::space::{LocalPosition, Quaternion, WorldPosition};

use crate::origin::{OriginEpoch as LocalOriginEpoch, OriginError, WorldOrigin};

/// Why a snapshot could not be published from the authoritative ledger.
///
/// A publication failure is reported, never patched: a value the declared
/// quantization cannot carry, or a lifecycle this schema cannot express, is a
/// fact about the session that the caller has to see.
#[derive(Clone, Debug, PartialEq)]
pub enum PublishError {
    /// The authoritative state itself is not publishable (non-finite or out of
    /// range).
    State(cs_sim::net_state::NetStateError),
    /// The world pose could not be expressed in the origin frame being
    /// published.
    Origin(OriginError),
    /// The snapshot schema refused the record.
    Snapshot(SnapshotError),
    /// The local origin epoch has no representable value in the snapshot
    /// schema's 32-bit epoch field, so no shared epoch can be named.
    EpochUnrepresentable {
        /// The local epoch that will not fit.
        local: u64,
    },
}

impl fmt::Display for PublishError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::State(error) => write!(f, "authoritative state is not publishable: {error}"),
            Self::Origin(error) => write!(f, "origin conversion rejected the pose: {error}"),
            Self::Snapshot(error) => write!(f, "snapshot schema refused the record: {error}"),
            Self::EpochUnrepresentable { local } => write!(
                f,
                "local origin epoch {local} does not fit the snapshot schema's 32-bit epoch field"
            ),
        }
    }
}

impl std::error::Error for PublishError {}

/// Publishes one authoritative actor's wire record.
///
/// This is the server side of the same schema the mirror reads: pose quantized
/// *relative to the published origin epoch*, velocities in their declared
/// budgets, and the essential flight/damage/weapon channels taken from the
/// server's own state. Nothing here takes a client packet, so a publication can
/// never carry client-authored authority.
///
/// The pose conversion is the one canonical conversion the local bodies use
/// ([`WorldOrigin::local_of`]): world identity stays f64 on this side and the
/// integer on the wire is that local frame through the declared quantization.
///
/// # Errors
///
/// [`PublishError::EpochUnrepresentable`] when the origin epoch does not fit the
/// schema, `Origin` when the pose cannot be expressed in that frame, `State` when
/// the authoritative state is not publishable, and `Snapshot` for any quantizer
/// or schema refusal.
pub fn publish_actor(
    state: &cs_sim::net_state::NetActorState,
    origin: &WorldOrigin,
) -> Result<ActorRecord, PublishError> {
    state.validate().map_err(PublishError::State)?;
    wire_epoch(origin.epoch()).map_err(|_| PublishError::EpochUnrepresentable {
        local: origin.epoch().0,
    })?;
    let local = origin
        .local_of(state.pose.position)
        .map_err(PublishError::Origin)?;
    let position = QuantizedVector::quantize(
        POSITION_QUANTIZATION,
        [
            f64::from(local.x()),
            f64::from(local.y()),
            f64::from(local.z()),
        ],
    )
    .map_err(PublishError::Snapshot)?;
    let rotation =
        QuantizedRotation::encode(state.pose.orientation).map_err(PublishError::Snapshot)?;
    let linear = QuantizedVector::quantize(LINEAR_VELOCITY_QUANTIZATION, state.linear_velocity_mps)
        .map_err(PublishError::Snapshot)?;
    let angular =
        QuantizedVector::quantize(ANGULAR_VELOCITY_QUANTIZATION, state.angular_velocity_radps)
            .map_err(PublishError::Snapshot)?;
    let rounds = |value: u32| -> Result<u16, PublishError> {
        u16::try_from(value).map_err(|_| {
            PublishError::Snapshot(SnapshotError::OutOfRange {
                field: "rounds",
                value: f64::from(value),
                max: f64::from(u16::MAX),
            })
        })
    };
    Ok(ActorRecord {
        actor: state.actor,
        generation: state.generation.get(),
        lifecycle: wire_lifecycle(state.lifecycle),
        control: wire_control(state.control),
        position,
        rotation,
        linear_velocity: linear,
        angular_velocity: angular,
        flight: FlightChannel::from_fractions(
            state.flight.throttle,
            state.flight.engine_spool,
            state.flight.boost_capacity,
        )
        .map_err(PublishError::Snapshot)?,
        damage: DamageChannel::from_integrity_fraction(
            state.damage.remaining,
            state.damage.disabled_mounts,
        )
        .map_err(PublishError::Snapshot)?,
        weapons: WeaponChannel::from_rounds(
            rounds(state.weapons.primary_rounds)?,
            rounds(state.weapons.secondary_rounds)?,
            wire_bank(state.weapons.selected_bank),
        )
        .map_err(PublishError::Snapshot)?,
    })
}

/// Publishes the whole ledger as one tick's snapshot.
///
/// The snapshot carries the shared origin epoch, the highest client input
/// sequence the ledger has consumed, and one record per tracked actor — so a
/// receiver can apply the tick and stop resending input without a second packet.
///
/// # Errors
///
/// Any [`publish_actor`] refusal for any tracked actor, plus the snapshot's own
/// validation refusals — which include the declared actor cap, so a population
/// that does not fit one snapshot is reported rather than truncated.
pub fn publish_snapshot(
    ledger: &cs_sim::net_state::NetStateLedger,
    origin: &WorldOrigin,
) -> Result<Snapshot, PublishError> {
    let epoch = wire_epoch(origin.epoch()).map_err(|_| PublishError::EpochUnrepresentable {
        local: origin.epoch().0,
    })?;
    let mut records = Vec::with_capacity(ledger.actor_count());
    for state in ledger.states() {
        records.push(publish_actor(state, origin)?);
    }
    let snapshot = Snapshot::new(epoch, ledger.acknowledged_input(), records);
    snapshot
        .validate(ledger.session())
        .map_err(PublishError::Snapshot)?;
    Ok(snapshot)
}

/// The wire lifecycle for one authoritative lifecycle.
///
/// A bailout and a mission removal both publish as [`Lifecycle::Despawned`]: the
/// wire flag answers "is this actor still here?", and the *reason* is a
/// server-side fact that travels as a reliable lifecycle event rather than a bit
/// in a droppable snapshot. F57-C owns that event; the original's own lifecycle
/// vocabulary is unmeasured, so no finer distinction is invented here.
fn wire_lifecycle(lifecycle: cs_sim::net_state::NetLifecycle) -> Lifecycle {
    match lifecycle {
        cs_sim::net_state::NetLifecycle::Alive => Lifecycle::Alive,
        cs_sim::net_state::NetLifecycle::Destroyed => Lifecycle::Destroyed,
        cs_sim::net_state::NetLifecycle::BailedOut | cs_sim::net_state::NetLifecycle::Despawned => {
            Lifecycle::Despawned
        }
    }
}

/// The wire control mode for one authoritative control mode.
fn wire_control(control: cs_sim::net_state::NetControlMode) -> ControlMode {
    match control {
        cs_sim::net_state::NetControlMode::Manual => ControlMode::Manual,
        cs_sim::net_state::NetControlMode::Autopilot => ControlMode::Autopilot,
        cs_sim::net_state::NetControlMode::Uncontrolled => ControlMode::Uncontrolled,
    }
}

/// The wire bank for one authoritative bank code.
fn wire_bank(code: u8) -> Bank {
    match code {
        1 => Bank::Secondary,
        _ => Bank::Primary,
    }
}

/// Why one snapshot or one record was refused by the ingest path.
///
/// Every refusal is named: a snapshot that silently fails to apply is
/// indistinguishable from one that applied.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum IngestRefusal {
    /// The snapshot's origin epoch is not the live world-origin epoch, so its
    /// relative coordinates cannot be interpreted. Refused rather than read
    /// against the wrong frame.
    EpochMismatch {
        /// The epoch the snapshot declares.
        snapshot: u32,
        /// The epoch the local world origin is in.
        local: u64,
    },
    /// The local origin epoch does not fit the snapshot schema's 32-bit epoch
    /// field, so no shared epoch can be named at all.
    EpochUnrepresentable {
        /// The local epoch that will not fit.
        local: u64,
    },
    /// The snapshot's tick is older than the newest snapshot already applied: a
    /// reordered late packet.
    OutOfOrder {
        /// The tick the snapshot declares.
        snapshot: u64,
        /// The newest tick applied so far.
        applied: u64,
    },
    /// The record's actor belongs to another session epoch.
    ForeignSession {
        /// The session the mirror belongs to.
        session: u64,
    },
    /// The record's generation is one that has already ended. A stale packet for
    /// a dead generation never comes back to life.
    GenerationEnded {
        /// The generation that already ended.
        generation: u16,
    },
    /// The actor was retired by a reliable `ActorRemoved`. Actor ids are never
    /// recycled, so no later record of any generation may apply.
    ReliablyRemoved,
    /// The record's rotation did not decode into a unit quaternion.
    UnusableRotation,
    /// The record's position could not be expressed in the local origin frame.
    UnusablePosition,
}

impl fmt::Display for IngestRefusal {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::EpochMismatch { snapshot, local } => write!(
                f,
                "snapshot is in origin epoch {snapshot}, but the local origin is in epoch {local}"
            ),
            Self::EpochUnrepresentable { local } => write!(
                f,
                "local origin epoch {local} does not fit the snapshot schema's 32-bit epoch field"
            ),
            Self::OutOfOrder { snapshot, applied } => write!(
                f,
                "snapshot tick {snapshot} is older than the applied tick {applied}"
            ),
            Self::ForeignSession { session } => write!(
                f,
                "snapshot record belongs to another session epoch than {session}"
            ),
            Self::GenerationEnded { generation } => write!(
                f,
                "record's generation {generation} has already ended and cannot be applied again"
            ),
            Self::ReliablyRemoved => write!(
                f,
                "actor was retired by a reliable removal; its id is never recycled"
            ),
            Self::UnusableRotation => {
                write!(f, "record's rotation did not decode into a unit quaternion")
            }
            Self::UnusablePosition => {
                write!(f, "record's position is not usable in this origin frame")
            }
        }
    }
}

/// Why a dequantized remote state could not be built.
///
/// Both variants mean "this record is untrustworthy"; which one is named so a
/// caller can tell a bad rotation from a position the origin frame cannot
/// express.
#[derive(Clone, Debug, PartialEq)]
pub enum MirrorError {
    /// A dequantized spatial value crossed the boundary and was rejected there.
    Origin(OriginError),
    /// The record's rotation did not decode into a unit quaternion.
    Rotation,
}

impl fmt::Display for MirrorError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Origin(error) => write!(f, "origin conversion rejected the record: {error}"),
            Self::Rotation => write!(f, "record rotation did not decode into a unit quaternion"),
        }
    }
}

impl std::error::Error for MirrorError {}

/// The snapshot schema's 32-bit epoch for a local 64-bit origin epoch.
///
/// The snapshot epoch field is the narrow one, so a local epoch beyond it has no
/// representable shared value. Rather than truncating — which would alias two
/// different frames onto one epoch id — this is a named refusal.
///
/// # Errors
///
/// [`IngestRefusal::EpochUnrepresentable`] when the local epoch does not fit.
fn wire_epoch(local: LocalOriginEpoch) -> Result<OriginEpoch, IngestRefusal> {
    u32::try_from(local.0)
        .map(OriginEpoch)
        .map_err(|_| IngestRefusal::EpochUnrepresentable { local: local.0 })
}

/// One remote aircraft's local mirror state, at one generation.
///
/// The generation is part of the record rather than a side table, so a consumer
/// cannot read a pose without also reading which generation it belongs to.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct RemoteAircraft {
    /// Which actor this is.
    pub actor: ActorId,
    /// Which generation of that actor this record belongs to. Every later stage
    /// keys its buffers on `(actor, generation)`.
    pub generation: u16,
    /// The tick whose snapshot produced this record.
    pub tick: Tick,
    /// Canonical world position in meters, reconstructed through the live origin
    /// epoch.
    pub position: WorldPosition,
    /// Body-to-world orientation.
    pub orientation: Quaternion,
    /// World linear velocity in m/s, dequantized.
    pub linear_velocity_mps: [f64; 3],
    /// Body angular velocity in rad/s, dequantized.
    pub angular_velocity_radps: [f64; 3],
    /// Flight channel fractions: throttle, engine spool, boost capacity.
    pub flight: [f64; 3],
    /// Remaining integrity as a fraction of full.
    pub integrity: f64,
    /// Bit mask of disabled weapon mounts.
    pub disabled_mounts: u16,
    /// Rounds per bank, primary first.
    pub rounds: [u16; 2],
    /// Which bank the trigger is on.
    pub selected_bank: u8,
    /// Who is steering.
    pub control: u8,
}

impl RemoteAircraft {
    /// Rebuilds the local mirror record from one wire record.
    ///
    /// The quantized position is dequantized in the snapshot's own frame and
    /// converted to canonical world coordinates through `origin` — the same
    /// single conversion the local bodies use, so a rebase of the origin moves a
    /// remote aircraft exactly as it moves a local one.
    ///
    /// # Errors
    ///
    /// [`MirrorError::Rotation`] when the rotation does not decode or a stored
    /// velocity does not fit its declared width, and [`MirrorError::Origin`]
    /// when the position cannot be expressed in the local frame.
    pub fn from_record(
        record: &ActorRecord,
        tick: Tick,
        origin: &WorldOrigin,
    ) -> Result<Self, MirrorError> {
        let relative = record.position_m().map_err(|_| MirrorError::Rotation)?;
        let orientation = record
            .rotation
            .decode()
            .map_err(|_| MirrorError::Rotation)?;
        let linear = record
            .linear_velocity_mps()
            .map_err(|_| MirrorError::Rotation)?;
        let angular = record
            .angular_velocity_radps()
            .map_err(|_| MirrorError::Rotation)?;
        let position = origin
            .world_of(
                LocalPosition::try_new([
                    relative[0] as f32,
                    relative[1] as f32,
                    relative[2] as f32,
                ])
                .map_err(|error| MirrorError::Origin(OriginError::Space(error)))?,
            )
            .map_err(MirrorError::Origin)?;
        Ok(Self {
            actor: record.actor,
            generation: record.generation,
            tick,
            position,
            orientation,
            linear_velocity_mps: linear,
            angular_velocity_radps: angular,
            flight: record.flight.fractions(),
            integrity: record.damage.integrity_fraction(),
            disabled_mounts: record.damage.disabled_mounts,
            rounds: [
                record.weapons.primary_rounds,
                record.weapons.secondary_rounds,
            ],
            selected_bank: record.weapons.selected.code(),
            control: record.control.code(),
        })
    }
}

/// What one ingest attempt did to the mirror as a whole.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum IngestOutcome {
    /// The snapshot's epoch matched, its tick was not older than the applied
    /// one, and at least one record was applied.
    Applied,
    /// The snapshot was refused whole; no record changed. The reason is the
    /// refusal attached to the report.
    Refused(IngestRefusal),
}

/// One snapshot's effect on the mirror.
///
/// A report is produced for every ingest attempt, including a refused one, so
/// "nothing happened here" is distinguishable from "nothing was looked at".
#[derive(Clone, Debug, PartialEq)]
pub struct IngestReport {
    /// What happened to the snapshot as a whole.
    pub outcome: IngestOutcome,
    /// The snapshot's tick.
    pub tick: Tick,
    /// The origin epoch the snapshot was read in.
    pub origin: OriginEpoch,
    /// How many records were applied to the mirror.
    pub applied: usize,
    /// Actors whose mirror record was created by this snapshot.
    pub spawned: Vec<ActorId>,
    /// Actors whose mirror record was updated by this snapshot.
    pub updated: Vec<ActorId>,
    /// Actors whose previous generation this snapshot retired because the record
    /// named a different generation.
    pub replaced: Vec<ActorId>,
    /// Actors this snapshot destroyed, in wire order. An actor's destruction is
    /// reported at most once for its generation: a later snapshot carrying the
    /// same terminal flag is refused as [`IngestRefusal::GenerationEnded`].
    pub destroyed: Vec<ActorId>,
    /// Actors this snapshot despawned without a kill.
    pub despawned: Vec<ActorId>,
    /// Per-record refusals, with the actor they belong to.
    pub refused: Vec<(ActorId, IngestRefusal)>,
}

impl IngestReport {
    /// A report for a snapshot refused whole.
    #[must_use]
    pub const fn refused(tick: Tick, origin: OriginEpoch, refusal: IngestRefusal) -> Self {
        Self {
            outcome: IngestOutcome::Refused(refusal),
            tick,
            origin,
            applied: 0,
            spawned: Vec::new(),
            updated: Vec::new(),
            replaced: Vec::new(),
            destroyed: Vec::new(),
            despawned: Vec::new(),
            refused: Vec::new(),
        }
    }

    /// Whether at least one record was applied.
    #[must_use]
    pub const fn is_applied(&self) -> bool {
        self.applied > 0
    }

    /// Whether this snapshot ended any actor.
    #[must_use]
    pub fn ended_any(&self) -> bool {
        !self.destroyed.is_empty() || !self.despawned.is_empty()
    }

    /// The whole-snapshot refusal, when this snapshot was refused.
    #[must_use]
    pub const fn whole_refusal(&self) -> Option<IngestRefusal> {
        match self.outcome {
            IngestOutcome::Refused(refusal) => Some(refusal),
            IngestOutcome::Applied => None,
        }
    }
}

/// The local mirror of every remote actor in one session.
///
/// The mirror is presentation state and nothing else: it holds no damage, no
/// ammunition ledger and no mission result, so it cannot award anything even by
/// accident (`docs/contracts/UI-NETWORK.md`, ownership table).
#[derive(Clone, Debug)]
pub struct RemoteMirror {
    session: SessionId,
    origin: WorldOrigin,
    applied_tick: Tick,
    actors: BTreeMap<ActorId, RemoteAircraft>,
    /// Generations that have ended through a record's own terminal lifecycle
    /// flag. Bounded by the actors the session ever spawned and dropped by
    /// [`Self::forget`], so a late packet can never resurrect an ended
    /// generation.
    ended: BTreeMap<ActorId, u16>,
    /// Actors a reliable `ActorRemoved` has retired for the whole session.
    ///
    /// Separate from `ended` because the two are different facts: a terminal
    /// snapshot flag ends *that generation*, while a reliable removal ends the
    /// actor id itself — actor serials are never recycled, so no later snapshot
    /// may bring that id back, whatever generation it names.
    removed: BTreeSet<ActorId>,
}

impl RemoteMirror {
    /// A mirror for `session`, reading records in `origin`'s frame.
    #[must_use]
    pub const fn new(session: SessionId, origin: WorldOrigin) -> Self {
        Self {
            session,
            origin,
            applied_tick: Tick(0),
            actors: BTreeMap::new(),
            ended: BTreeMap::new(),
            removed: BTreeSet::new(),
        }
    }

    /// The session this mirror belongs to.
    #[must_use]
    pub const fn session(&self) -> SessionId {
        self.session
    }

    /// The world origin this mirror reads records in.
    #[must_use]
    pub const fn origin(&self) -> &WorldOrigin {
        &self.origin
    }

    /// The newest snapshot tick applied.
    #[must_use]
    pub const fn applied_tick(&self) -> Tick {
        self.applied_tick
    }

    /// The mirror record for `actor`.
    #[must_use]
    pub fn aircraft(&self, actor: ActorId) -> Option<&RemoteAircraft> {
        self.actors.get(&actor)
    }

    /// How many remote aircraft the mirror currently shows.
    #[must_use]
    pub fn aircraft_count(&self) -> usize {
        self.actors.len()
    }

    /// Every mirror record, in `ActorId` order.
    pub fn all_aircraft(&self) -> impl Iterator<Item = &RemoteAircraft> {
        self.actors.values()
    }

    /// Applies one decoded snapshot.
    ///
    /// Check order is the contract: session, then shared origin epoch, then tick
    /// ordering, then per record. A snapshot failing any of the first three
    /// changes nothing at all.
    pub fn ingest(&mut self, snapshot: &Snapshot, tick: Tick) -> IngestReport {
        let local_epoch = self.origin.epoch();
        let refusal = match wire_epoch(local_epoch) {
            Err(refusal) => refusal,
            Ok(epoch) if epoch != snapshot.origin => IngestRefusal::EpochMismatch {
                snapshot: snapshot.origin.0,
                local: local_epoch.0,
            },
            Ok(_) if tick < self.applied_tick => IngestRefusal::OutOfOrder {
                snapshot: tick.0,
                applied: self.applied_tick.0,
            },
            Ok(_) => return self.apply_records(snapshot, tick),
        };
        IngestReport::refused(tick, snapshot.origin, refusal)
    }

    /// Applies a snapshot whose epoch and tick have already been checked.
    fn apply_records(&mut self, snapshot: &Snapshot, tick: Tick) -> IngestReport {
        let mut report = IngestReport {
            outcome: IngestOutcome::Applied,
            tick,
            origin: snapshot.origin,
            applied: 0,
            spawned: Vec::new(),
            updated: Vec::new(),
            replaced: Vec::new(),
            destroyed: Vec::new(),
            despawned: Vec::new(),
            refused: Vec::new(),
        };
        for record in &snapshot.actors {
            if record.actor.session != self.session {
                report.refused.push((
                    record.actor,
                    IngestRefusal::ForeignSession {
                        session: self.session.get(),
                    },
                ));
                continue;
            }
            if self.removed.contains(&record.actor) {
                report
                    .refused
                    .push((record.actor, IngestRefusal::ReliablyRemoved));
                continue;
            }
            if self.ended.get(&record.actor) == Some(&record.generation) {
                report.refused.push((
                    record.actor,
                    IngestRefusal::GenerationEnded {
                        generation: record.generation,
                    },
                ));
                continue;
            }
            let previous = self.actors.get(&record.actor).copied();
            let same_generation =
                previous.is_some_and(|current| current.generation == record.generation);
            if let Some(current) = previous
                && !same_generation
            {
                // A generation change retires the old record outright: nothing of
                // it survives into the new one.
                self.actors.remove(&record.actor);
                self.ended.insert(record.actor, current.generation);
                report.replaced.push(record.actor);
            }
            let mirror = match RemoteAircraft::from_record(record, tick, &self.origin) {
                Ok(mirror) => mirror,
                Err(error) => {
                    report.refused.push((record.actor, refusal_for(error)));
                    continue;
                }
            };
            report.applied += 1;
            if same_generation {
                report.updated.push(record.actor);
            } else {
                report.spawned.push(record.actor);
            }
            // A terminal lifecycle ends the record here, on the wire's own
            // authority. Absence from a later snapshot is never an end: sequenced
            // snapshots are droppable.
            match record.lifecycle {
                Lifecycle::Alive => {
                    self.actors.insert(record.actor, mirror);
                }
                Lifecycle::Destroyed => {
                    self.actors.remove(&record.actor);
                    self.ended.insert(record.actor, record.generation);
                    report.destroyed.push(record.actor);
                }
                Lifecycle::Despawned => {
                    self.actors.remove(&record.actor);
                    self.ended.insert(record.actor, record.generation);
                    report.despawned.push(record.actor);
                }
            }
        }
        self.applied_tick = tick;
        report
    }

    /// Applies one reliable, idempotent session event.
    ///
    /// Only lifecycle facts are consumed: an [`EventBody::ActorRemoved`] retires
    /// the actor for the whole session, so a *replayed* reliable event (reconnect
    /// and retry can replay a request) is absorbed instead of retiring an actor a
    /// second time, and a snapshot that carries the same removal afterwards — or
    /// one that arrives with a different generation — cannot bring the id back.
    /// Every other event is observed and ignored: the mirror is presentation
    /// state and runs no mission logic.
    ///
    /// Returns `true` when this call is the one that recorded the removal, so a
    /// replay is distinguishable from the first application.
    #[must_use]
    pub fn apply_event(&mut self, event: &ReliableEvent) -> bool {
        let EventBody::ActorRemoved { actor } = event.body else {
            return false;
        };
        if actor.session != self.session {
            return false;
        }
        self.actors.remove(&actor);
        self.ended.remove(&actor);
        self.removed.insert(actor)
    }

    /// Drops an actor and its ended-generation record.
    ///
    /// The only way the mirror forgets that a generation ended. Callers hold this
    /// for the moment the rest of the app releases a session's actors; until then
    /// the record is what makes a late packet harmless.
    pub fn forget(&mut self, actor: ActorId) {
        self.actors.remove(&actor);
        self.ended.remove(&actor);
        self.removed.remove(&actor);
    }
}

/// The per-record refusal that names why a record could not be mirrored.
fn refusal_for(error: MirrorError) -> IngestRefusal {
    match error {
        MirrorError::Rotation => IngestRefusal::UnusableRotation,
        MirrorError::Origin(_) => IngestRefusal::UnusablePosition,
    }
}
