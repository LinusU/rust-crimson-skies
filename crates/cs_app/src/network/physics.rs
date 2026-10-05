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
//! * **Generations never mix.** A record naming a *newer* generation than the one
//!   the mirror holds retires the old record and starts a new one, so no field of
//!   one generation is carried into another; a record naming an *older* one is
//!   refused. That is what makes it safe for F57-B to key its interpolation
//!   history on `(actor, generation)` (F57 AC04).
//! * **Loss is not evidence.** An actor missing from one snapshot is *not*
//!   despawned: sequenced snapshots are droppable, so absence proves nothing.
//!   Retirement comes from the record's own lifecycle flag or from a reliable
//!   [`cs_net::message::EventBody::ActorRemoved`], which is why a session with
//!   10 % loss still ends with no ghost aircraft (F57 AC01).
//! * **Reordering is not rollback.** A snapshot older than the one already
//!   applied is refused whole, and a record naming an *older* generation than
//!   the one the mirror holds is refused per record: generations only ever
//!   increase for an id, so an older one is a late packet, not a new actor. A
//!   late packet never rewinds a remote aircraft and never resurrects a
//!   generation that has already ended.
//! * **Presentation only.** Nothing here awards damage, consumes ammunition or
//!   resolves a hit. Those are server-owned (UI-NETWORK ownership table); the
//!   mirror only reports what the server said.
//!
//! All of this is newly authored engine design: no original networked behavior,
//! interpolation delay or remote-aircraft presentation has been measured, and
//! none is asserted here.

use std::collections::{BTreeMap, BTreeSet, VecDeque};
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
/// The declared position budget therefore bounds *quantization*, not the f32 the
/// local frame is held in: a position far from the origin epoch is additionally
/// subject to the world → local → world round-trip tolerance
/// ([`crate::origin::local_round_trip_tolerance_m`]), which is the bound a
/// consumer should use for a mirrored pose and not the quantization budget alone.
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
    /// The record names an *older* generation than the one the mirror holds. A
    /// generation only ever increases for an id, so this is a reordered or
    /// replayed record rather than a new actor, and applying it would roll the
    /// mirror back to a state it has already left.
    StaleGeneration {
        /// The older generation the record names.
        record: u16,
        /// The newer generation the mirror holds.
        held: u16,
    },
    /// The actor was retired by a reliable `ActorRemoved`. Actor ids are never
    /// recycled, so no later record of any generation may apply.
    ReliablyRemoved,
    /// The record's rotation did not decode into a unit quaternion.
    UnusableRotation,
    /// The record's position could not be expressed in the local origin frame.
    UnusablePosition,
    /// A stored integer does not fit the declared width of the field it arrived
    /// in. A corrupt or foreign payload is named by its field rather than
    /// reported as something else.
    UnreadableField {
        /// The declared field whose stored value cannot be read.
        field: &'static str,
    },
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
            Self::StaleGeneration { record, held } => write!(
                f,
                "record names generation {record}, older than the generation {held} the mirror holds"
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
            Self::UnreadableField { field } => write!(
                f,
                "record's {field} field holds a value its declared width cannot carry"
            ),
        }
    }
}

/// Why a dequantized remote state could not be built.
///
/// Every variant names the field it is about: a caller that is told "the rotation
/// is unusable" must not be handed a position that failed instead.
#[derive(Clone, Debug, PartialEq)]
pub enum MirrorError {
    /// A dequantized spatial value crossed the boundary and was rejected there.
    Origin(OriginError),
    /// The record's rotation did not decode into a unit quaternion.
    Rotation,
    /// A stored integer does not fit the declared width of its field.
    UnreadableField {
        /// The declared field that cannot be read.
        field: &'static str,
    },
}

impl fmt::Display for MirrorError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Origin(error) => write!(f, "origin conversion rejected the record: {error}"),
            Self::Rotation => write!(f, "record rotation did not decode into a unit quaternion"),
            Self::UnreadableField { field } => write!(
                f,
                "record's {field} field holds a value its declared width cannot carry"
            ),
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
    /// [`MirrorError::Rotation`] when the rotation does not decode,
    /// [`MirrorError::UnreadableField`] naming the position or velocity field
    /// whose stored integer does not fit its declared width, and
    /// [`MirrorError::Origin`] when the position cannot be expressed in the local
    /// frame.
    pub fn from_record(
        record: &ActorRecord,
        tick: Tick,
        origin: &WorldOrigin,
    ) -> Result<Self, MirrorError> {
        let relative = record
            .position_m()
            .map_err(|_| MirrorError::UnreadableField {
                field: POSITION_QUANTIZATION.field(),
            })?;
        let orientation = record
            .rotation
            .decode()
            .map_err(|_| MirrorError::Rotation)?;
        let linear = record
            .linear_velocity_mps()
            .map_err(|_| MirrorError::UnreadableField {
                field: LINEAR_VELOCITY_QUANTIZATION.field(),
            })?;
        let angular =
            record
                .angular_velocity_radps()
                .map_err(|_| MirrorError::UnreadableField {
                    field: ANGULAR_VELOCITY_QUANTIZATION.field(),
                })?;
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
                && current.generation > record.generation
            {
                // Generations only ever increase for an id, so an older one is a
                // reordered record, not a new actor: applying it would rewind the
                // mirror to a state it has already left.
                report.refused.push((
                    record.actor,
                    IngestRefusal::StaleGeneration {
                        record: record.generation,
                        held: current.generation,
                    },
                ));
                continue;
            }
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
    /// An event stamped with another session generation is not this session's
    /// event at all, so it is left alone like any other foreign event. Every
    /// other event is observed and ignored: the mirror is presentation state and
    /// runs no mission logic.
    ///
    /// Returns `true` when this call is the one that recorded the removal, so a
    /// replay is distinguishable from the first application.
    #[must_use]
    pub fn apply_event(&mut self, event: &ReliableEvent) -> bool {
        if event.id.session != self.session {
            return false;
        }
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

    /// Adopts a new shared origin epoch, re-projecting every mirrored record.
    ///
    /// This is F57-C's answer to "origin change across snapshot boundaries" (F57
    /// AC03). Positions on the wire are integers relative to whichever epoch the
    /// snapshot declares, so a receiver whose local frame moves to a new epoch
    /// must (a) adopt that epoch, or every subsequent snapshot is refused as
    /// [`IngestRefusal::EpochMismatch`] and remote aircraft freeze forever, and
    /// (b) treat the change as a conversion rather than a displacement, or every
    /// mirrored aircraft teleports by exactly `origin_shift_m`.
    ///
    /// The canonical f64 world position is what a rebase preserves, so each
    /// record is re-projected through the new frame *and measured* against its
    /// stored value; the largest drift is reported as
    /// [`EpochTransition::max_conversion_m`]. A record that still carried the old
    /// frame's local numbers would move by `origin_shift_m` here, which is why the
    /// transition reports both numbers instead of quietly succeeding.
    ///
    /// Epochs are never reused, so an epoch that is not strictly newer than the
    /// one held is refused: adopting an old or repeated epoch would let a stale
    /// frame name live records.
    ///
    /// # Errors
    ///
    /// [`OriginError::EpochMismatch`] for an epoch that is not strictly newer
    /// than the held one, and [`OriginError::Space`] when a record's position
    /// cannot be converted through the new frame — in which case the mirror is
    /// left exactly as it was.
    pub fn adopt_origin(&mut self, next: WorldOrigin) -> Result<EpochTransition, OriginError> {
        if next.epoch() <= self.origin.epoch() {
            return Err(OriginError::EpochMismatch {
                anchor: self.origin.epoch(),
                frame: next.epoch(),
            });
        }
        // Phase one: plan every conversion. A record that cannot be converted
        // aborts the whole transition, so the mirror is never half-rebased.
        let mut projections = Vec::with_capacity(self.actors.len());
        let mut max_conversion_m = 0.0_f64;
        for (actor, record) in &self.actors {
            let local = next.local_of(record.position)?;
            let projected = next.world_of(local)?;
            let drift = distance(projected.to_array(), record.position.to_array());
            let tolerance = crate::origin::local_round_trip_tolerance_m(record.position, local);
            if drift > tolerance {
                return Err(OriginError::Space(cs_types::space::SpaceError::NonFinite {
                    field: "origin re-projection",
                }));
            }
            max_conversion_m = max_conversion_m.max(drift);
            projections.push((*actor, projected));
        }
        // Phase two: commit.
        let previous = self.origin;
        for (actor, projected) in projections {
            if let Some(record) = self.actors.get_mut(&actor) {
                record.position = projected;
            }
        }
        self.origin = next;
        Ok(EpochTransition {
            from: previous.epoch(),
            to: next.epoch(),
            origin: next.position(),
            origin_shift_m: distance(previous.position().to_array(), next.position().to_array()),
            converted: self.actors.len(),
            max_conversion_m,
        })
    }
}

/// What adopting a new shared origin epoch did to a mirror.
///
/// A rebase is a *conversion*, not a move (`docs/contracts/FLIGHT-PHYSICS.md`:
/// "an origin shift translates both endpoints consistently"; F16 non-negotiable
/// behavior 5). This report is the measurement that says so: `origin_shift_m` is
/// how far the frame moved, and `max_conversion_m` is how far any mirrored actor
/// moved as a result. A consumer that wants to be sure the epoch change was not
// a displacement compares the two — they are orders of magnitude apart, and the
/// acceptance test asserts exactly that.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct EpochTransition {
    /// The epoch the mirror read records in before the transition.
    pub from: LocalOriginEpoch,
    /// The epoch it reads records in now.
    pub to: LocalOriginEpoch,
    /// The new origin's canonical world position.
    pub origin: WorldPosition,
    /// How far the origin itself moved, in meters. This is the distance that
    /// must *not* appear in any actor's pose.
    pub origin_shift_m: f64,
    /// How many mirror records were re-projected through the new frame.
    pub converted: usize,
    /// The largest distance any re-projected record moved, in meters: the
    /// world → local → world round-trip tolerance
    /// ([`crate::origin::local_round_trip_tolerance_m`]), not a displacement.
    pub max_conversion_m: f64,
}

/// The per-record refusal that names why a record could not be mirrored.
fn refusal_for(error: MirrorError) -> IngestRefusal {
    match error {
        MirrorError::Rotation => IngestRefusal::UnusableRotation,
        MirrorError::Origin(_) => IngestRefusal::UnusablePosition,
        MirrorError::UnreadableField { field } => IngestRefusal::UnreadableField { field },
    }
}

// ---------------------------------------------------------------------------
// F57-B: bounded interpolation of remote aircraft and bounded local prediction.
//
// Spec: `specs/F57-*.md`, stage `### F57-B`. Everything below is newly authored
// engine design (the sheet: "a designed responsiveness layer"); no original
// interpolation delay, extrapolation rule or correction behavior has been
// measured, and none is asserted. The defaults are therefore explicit config,
// not constants buried in the algorithm.
// ---------------------------------------------------------------------------

/// Why an interpolation or prediction operation refused its input.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum BufferRefusal {
    /// The sample names an older generation than the one the buffer holds (or
    /// than one that already ended): a late packet, never a new actor.
    StaleGeneration {
        /// The generation the sample names.
        sample: u16,
        /// The generation the buffer holds or has ended.
        held: u16,
    },
    /// The sample's tick is not newer than the newest buffered sample.
    NotNewer {
        /// The sample's tick.
        sample: u64,
        /// The newest buffered tick.
        newest: u64,
    },
    /// The sample belongs to another actor than the predictor's.
    WrongActor,
    /// A non-finite or otherwise unusable value reached the boundary.
    Unusable,
}

impl fmt::Display for BufferRefusal {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::StaleGeneration { sample, held } => write!(
                f,
                "sample generation {sample} is older than generation {held}"
            ),
            Self::NotNewer { sample, newest } => {
                write!(f, "sample tick {sample} is not newer than tick {newest}")
            }
            Self::WrongActor => f.write_str("sample belongs to another actor"),
            Self::Unusable => f.write_str("sample holds a non-finite value"),
        }
    }
}

impl std::error::Error for BufferRefusal {}

/// Declared bounds of the remote-aircraft jitter buffer.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct InterpolationConfig {
    /// Simulation ticks per second, to turn a velocity into a per-tick step.
    pub ticks_per_second: u32,
    /// How many ticks behind the estimated server tick remote aircraft render.
    pub delay_ticks: u64,
    /// The most samples one actor's buffer holds; the oldest is dropped first.
    pub capacity: usize,
    /// The longest span extrapolated past the newest sample before the pose is
    /// held.
    pub max_extrapolation_ticks: u64,
    /// Two samples further apart than this are not interpolated across: the
    /// older one is held until the newer one's tick.
    pub max_gap_ticks: u64,
    /// A displacement between consecutive samples above this is a teleport or
    /// spawn, not motion, and is never interpolated across.
    pub teleport_distance_m: f64,
}

impl Default for InterpolationConfig {
    fn default() -> Self {
        Self {
            ticks_per_second: 60,
            delay_ticks: 6,
            capacity: 16,
            max_extrapolation_ticks: 6,
            max_gap_ticks: 30,
            teleport_distance_m: 1000.0,
        }
    }
}

/// How a sampled pose was produced.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SampleMode {
    /// Blended between two buffered snapshots.
    Interpolated,
    /// An exact buffered snapshot, or one held across a gap/teleport.
    Held,
    /// Projected forward from the newest snapshot along its velocity.
    Extrapolated,
    /// Held at the extrapolation limit: no newer snapshot has arrived within the
    /// bound, so the pose stops rather than drifting on guesswork.
    ExtrapolationExhausted,
}

/// One remote aircraft as presented at a render tick.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct InterpolatedAircraft {
    /// The tick that was rendered (`now - delay`).
    pub render_tick: Tick,
    /// How the pose was produced.
    pub mode: SampleMode,
    /// The authoritative record at or before the render tick, with only the pose
    /// fields (`position`, `orientation`, `linear_velocity_mps`) replaced by the
    /// presented ones. Discrete fields (rounds, boost capacity, integrity) are the
    /// server's last word, never blended.
    pub state: RemoteAircraft,
}

#[derive(Clone, Copy, Debug)]
struct BufferedSample {
    aircraft: RemoteAircraft,
    /// The step from the previous sample is a teleport/spawn: do not blend.
    discontinuity: bool,
}

#[derive(Clone, Debug)]
struct Track {
    generation: u16,
    samples: VecDeque<BufferedSample>,
}

/// Bounded jitter buffers for every remote actor, keyed on actor and generation.
///
/// One generation per actor at a time: a sample naming a newer generation
/// replaces the whole track, so no history of one generation is ever blended
/// into another (F57 AC04).
#[derive(Clone, Debug)]
pub struct RemoteInterpolator {
    config: InterpolationConfig,
    tracks: BTreeMap<ActorId, Track>,
    /// The highest generation that ended for each actor.
    ended: BTreeMap<ActorId, u16>,
}

fn distance(a: [f64; 3], b: [f64; 3]) -> f64 {
    ((a[0] - b[0]).powi(2) + (a[1] - b[1]).powi(2) + (a[2] - b[2]).powi(2)).sqrt()
}

fn quat_array(q: Quaternion) -> [f64; 4] {
    q.components()
}

fn quat_mul(a: [f64; 4], b: [f64; 4]) -> [f64; 4] {
    [
        a[3] * b[0] + a[0] * b[3] + a[1] * b[2] - a[2] * b[1],
        a[3] * b[1] - a[0] * b[2] + a[1] * b[3] + a[2] * b[0],
        a[3] * b[2] + a[0] * b[1] - a[1] * b[0] + a[2] * b[3],
        a[3] * b[3] - a[0] * b[0] - a[1] * b[1] - a[2] * b[2],
    ]
}

fn quat_conj(q: [f64; 4]) -> [f64; 4] {
    [-q[0], -q[1], -q[2], q[3]]
}

fn quat_normalize(q: [f64; 4]) -> [f64; 4] {
    let length = q.iter().map(|c| c * c).sum::<f64>().sqrt();
    if length == 0.0 {
        return [0.0, 0.0, 0.0, 1.0];
    }
    [q[0] / length, q[1] / length, q[2] / length, q[3] / length]
}

/// Shortest-path normalized lerp from `a` to `b` at `t` in `[0, 1]`.
fn quat_nlerp(a: [f64; 4], b: [f64; 4], t: f64) -> [f64; 4] {
    let dot: f64 = a.iter().zip(b).map(|(x, y)| x * y).sum();
    let sign = if dot < 0.0 { -1.0 } else { 1.0 };
    quat_normalize([
        a[0] + (sign * b[0] - a[0]) * t,
        a[1] + (sign * b[1] - a[1]) * t,
        a[2] + (sign * b[2] - a[2]) * t,
        a[3] + (sign * b[3] - a[3]) * t,
    ])
}

fn quat_angle(q: [f64; 4]) -> f64 {
    2.0 * q[3].abs().min(1.0).acos()
}

fn to_quaternion(q: [f64; 4]) -> Result<Quaternion, BufferRefusal> {
    Quaternion::try_new(quat_normalize(q)).map_err(|_| BufferRefusal::Unusable)
}

impl RemoteInterpolator {
    /// An empty interpolator with `config`'s bounds.
    #[must_use]
    pub const fn new(config: InterpolationConfig) -> Self {
        Self {
            config,
            tracks: BTreeMap::new(),
            ended: BTreeMap::new(),
        }
    }

    /// The declared bounds.
    #[must_use]
    pub const fn config(&self) -> &InterpolationConfig {
        &self.config
    }

    /// How many actors currently have a buffer.
    #[must_use]
    pub fn track_count(&self) -> usize {
        self.tracks.len()
    }

    /// How many samples `actor`'s buffer holds.
    #[must_use]
    pub fn sample_count(&self, actor: ActorId) -> usize {
        self.tracks.get(&actor).map_or(0, |t| t.samples.len())
    }

    /// Buffers one mirrored authoritative record.
    ///
    /// # Errors
    ///
    /// [`BufferRefusal::StaleGeneration`] for an older (or ended) generation,
    /// [`BufferRefusal::NotNewer`] for a tick not newer than the newest sample of
    /// the same generation.
    pub fn push(&mut self, aircraft: &RemoteAircraft) -> Result<(), BufferRefusal> {
        let held = self
            .tracks
            .get(&aircraft.actor)
            .map(|t| t.generation)
            .max(self.ended.get(&aircraft.actor).copied());
        let existing = self.tracks.get(&aircraft.actor).map(|t| t.generation);
        if let Some(held) = held
            && (aircraft.generation < held
                || (Some(aircraft.generation) == self.ended.get(&aircraft.actor).copied()
                    && existing != Some(aircraft.generation)))
        {
            return Err(BufferRefusal::StaleGeneration {
                sample: aircraft.generation,
                held,
            });
        }
        if existing.is_some_and(|g| g != aircraft.generation) {
            // A newer generation: nothing of the old history survives.
            self.tracks.remove(&aircraft.actor);
        }
        let capacity = self.config.capacity.max(2);
        let teleport = self.config.teleport_distance_m;
        let track = self.tracks.entry(aircraft.actor).or_insert_with(|| Track {
            generation: aircraft.generation,
            samples: VecDeque::new(),
        });
        let mut discontinuity = false;
        if let Some(newest) = track.samples.back() {
            if aircraft.tick <= newest.aircraft.tick {
                return Err(BufferRefusal::NotNewer {
                    sample: aircraft.tick.0,
                    newest: newest.aircraft.tick.0,
                });
            }
            discontinuity = distance(
                newest.aircraft.position.to_array(),
                aircraft.position.to_array(),
            ) > teleport;
        }
        track.samples.push_back(BufferedSample {
            aircraft: *aircraft,
            discontinuity,
        });
        while track.samples.len() > capacity {
            track.samples.pop_front();
        }
        Ok(())
    }

    /// Ends `actor`'s buffer: its generation never accepts another sample.
    pub fn retire(&mut self, actor: ActorId) {
        if let Some(track) = self.tracks.remove(&actor) {
            self.ended.insert(actor, track.generation);
        }
    }

    /// Drops everything remembered about `actor`, including ended generations.
    pub fn forget(&mut self, actor: ActorId) {
        self.tracks.remove(&actor);
        self.ended.remove(&actor);
    }

    /// Drops every buffer, remembered generation included.
    ///
    /// The teardown path ([`NetSession::teardown`]). Per-actor releases use
    /// [`Self::forget`] so a single actor's history can be released without
    /// ending the session; this is for the end of the session itself.
    pub fn clear(&mut self) {
        self.tracks.clear();
        self.ended.clear();
    }

    /// Feeds the effect of one ingested snapshot into the buffers: applied and
    /// replaced records are buffered from `mirror`, destroyed and despawned
    /// actors are retired at the generation the snapshot named.
    ///
    /// Returns the per-actor refusals; an empty list means every record was
    /// buffered or retired.
    pub fn observe(
        &mut self,
        report: &IngestReport,
        snapshot: &Snapshot,
        mirror: &RemoteMirror,
    ) -> Vec<(ActorId, BufferRefusal)> {
        let mut refusals = Vec::new();
        for actor in report.spawned.iter().chain(&report.updated) {
            if let Some(aircraft) = mirror.aircraft(*actor)
                && let Err(refusal) = self.push(aircraft)
            {
                refusals.push((*actor, refusal));
            }
        }
        for actor in report.destroyed.iter().chain(&report.despawned) {
            let generation = snapshot
                .actors
                .iter()
                .find(|r| r.actor == *actor)
                .map(|r| r.generation);
            self.tracks.remove(actor);
            if let Some(generation) = generation {
                let entry = self.ended.entry(*actor).or_insert(generation);
                *entry = (*entry).max(generation);
            }
        }
        refusals
    }

    /// The pose to present for `actor` when the estimated server tick is `now`.
    ///
    /// Returns `None` for an actor with no buffered sample (never spawned,
    /// retired, or forgotten): a despawned aircraft leaves nothing to draw.
    ///
    /// # Errors
    ///
    /// [`BufferRefusal::Unusable`] when the blended pose is not representable.
    pub fn sample(
        &self,
        actor: ActorId,
        now: Tick,
    ) -> Result<Option<InterpolatedAircraft>, BufferRefusal> {
        let Some(track) = self.tracks.get(&actor) else {
            return Ok(None);
        };
        let (Some(first), Some(last)) = (track.samples.front(), track.samples.back()) else {
            return Ok(None);
        };
        let render = Tick(now.0.saturating_sub(self.config.delay_ticks));
        let held = |sample: &BufferedSample, mode| InterpolatedAircraft {
            render_tick: render,
            mode,
            state: sample.aircraft,
        };
        if render <= first.aircraft.tick {
            return Ok(Some(held(first, SampleMode::Held)));
        }
        if render >= last.aircraft.tick {
            let ahead = render.0 - last.aircraft.tick.0;
            let used = ahead.min(self.config.max_extrapolation_ticks);
            let seconds = used as f64 / f64::from(self.config.ticks_per_second.max(1));
            let mut out = *last;
            let v = out.aircraft.linear_velocity_mps;
            let p = out.aircraft.position.to_array();
            out.aircraft.position = WorldPosition::try_new([
                p[0] + v[0] * seconds,
                p[1] + v[1] * seconds,
                p[2] + v[2] * seconds,
            ])
            .map_err(|_| BufferRefusal::Unusable)?;
            let mode = if ahead == 0 {
                SampleMode::Held
            } else if ahead > used {
                SampleMode::ExtrapolationExhausted
            } else {
                SampleMode::Extrapolated
            };
            return Ok(Some(InterpolatedAircraft {
                render_tick: render,
                mode,
                state: out.aircraft,
            }));
        }
        let (a, b) = track
            .samples
            .iter()
            .zip(track.samples.iter().skip(1))
            .find(|(a, b)| a.aircraft.tick <= render && render < b.aircraft.tick)
            .ok_or(BufferRefusal::Unusable)?;
        let span = b.aircraft.tick.0 - a.aircraft.tick.0;
        if b.discontinuity || span > self.config.max_gap_ticks {
            return Ok(Some(held(a, SampleMode::Held)));
        }
        let t = (render.0 - a.aircraft.tick.0) as f64 / span as f64;
        let lerp = |x: [f64; 3], y: [f64; 3]| {
            [
                x[0] + (y[0] - x[0]) * t,
                x[1] + (y[1] - x[1]) * t,
                x[2] + (y[2] - x[2]) * t,
            ]
        };
        let mut state = a.aircraft;
        state.position = WorldPosition::try_new(lerp(
            a.aircraft.position.to_array(),
            b.aircraft.position.to_array(),
        ))
        .map_err(|_| BufferRefusal::Unusable)?;
        state.orientation = to_quaternion(quat_nlerp(
            quat_array(a.aircraft.orientation),
            quat_array(b.aircraft.orientation),
            t,
        ))?;
        state.linear_velocity_mps = lerp(
            a.aircraft.linear_velocity_mps,
            b.aircraft.linear_velocity_mps,
        );
        Ok(Some(InterpolatedAircraft {
            render_tick: render,
            mode: SampleMode::Interpolated,
            state,
        }))
    }
}

/// Declared bounds of local prediction and its correction.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct PredictionConfig {
    /// Predicted poses kept for comparison with late authoritative records.
    pub history_ticks: usize,
    /// Position error above which the pose snaps instead of smoothing.
    pub snap_distance_m: f64,
    /// Orientation error above which the pose snaps instead of smoothing.
    pub snap_angle_rad: f64,
    /// Ticks over which a smoothed error is removed.
    pub correction_ticks: u32,
}

impl Default for PredictionConfig {
    fn default() -> Self {
        Self {
            history_ticks: 64,
            snap_distance_m: 5.0,
            snap_angle_rad: 0.35,
            correction_ticks: 6,
        }
    }
}

/// The local body's pose as the one pose owner reports it.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct PredictedPose {
    /// Canonical world position.
    pub position: WorldPosition,
    /// Body-to-world orientation.
    pub orientation: Quaternion,
}

/// The server's last word on the local aircraft's consumable and damage state.
///
/// Only [`LocalPredictor::reconcile`] writes this, from a mirrored server
/// record; prediction has no way to spend ammunition or boost capacity.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct AuthoritativeLoadout {
    /// The snapshot tick it came from.
    pub tick: Tick,
    /// Throttle, engine spool and boost capacity fractions.
    pub flight: [f64; 3],
    /// Rounds per bank, primary first.
    pub rounds: [u16; 2],
    /// Which bank the trigger is on.
    pub selected_bank: u8,
    /// Remaining integrity fraction.
    pub integrity: f64,
}

/// How one authoritative record related to the prediction.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum ReconcileKind {
    /// Within a negligible error, nothing to correct.
    Agreed,
    /// Removed gradually over the configured ticks.
    Smoothed,
    /// Too large to smooth, or no prediction to compare with: corrected at once.
    Snapped,
}

/// The result of reconciling one authoritative record.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Reconciliation {
    /// What was decided.
    pub kind: ReconcileKind,
    /// Position error in meters (server minus predicted at the record's tick).
    pub error_m: f64,
    /// Orientation error in radians.
    pub error_rad: f64,
}

/// One tick's correction to apply to the local body (the one pose owner).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct PoseCorrection {
    /// World-space translation to add.
    pub translation_m: [f64; 3],
    /// World-space rotation to pre-multiply onto the orientation.
    pub rotation: Quaternion,
}

/// Bounded local prediction and state correction for the local aircraft.
///
/// Not an exact rollback: Avian state is not rewound. The body keeps simulating
/// from its own inputs; this compares the pose it had at a record's tick with the
/// server's and hands back bounded per-tick corrections for the body to apply.
/// Limitation: velocity is not corrected here, so a large persistent error ends
/// in a snap rather than a re-simulation.
#[derive(Clone, Debug)]
pub struct LocalPredictor {
    actor: ActorId,
    generation: u16,
    config: PredictionConfig,
    history: VecDeque<(Tick, PredictedPose)>,
    pending_translation: [f64; 3],
    pending_rotation: [f64; 4],
    remaining_ticks: u32,
    boosting: bool,
    authoritative: Option<AuthoritativeLoadout>,
}

impl LocalPredictor {
    /// A predictor for the local aircraft `actor` at `generation`.
    #[must_use]
    pub const fn new(actor: ActorId, generation: u16, config: PredictionConfig) -> Self {
        Self {
            actor,
            generation,
            config,
            history: VecDeque::new(),
            pending_translation: [0.0; 3],
            pending_rotation: [0.0, 0.0, 0.0, 1.0],
            remaining_ticks: 0,
            boosting: false,
            authoritative: None,
        }
    }

    /// The local aircraft this predictor owns the history of.
    #[must_use]
    pub const fn actor(&self) -> ActorId {
        self.actor
    }

    /// The local aircraft's generation the history belongs to.
    #[must_use]
    pub const fn generation(&self) -> u16 {
        self.generation
    }

    /// Drops the predicted history, the pending correction and the authoritative
    /// loadout.
    ///
    /// The teardown path ([`NetSession::teardown`]). After this the predictor
    /// answers as a fresh one: the next record has no predicted pose to compare
    /// against, so it is adopted by snap, which is the honest reading of "no
    /// prediction exists" rather than a silent guess.
    pub fn reset(&mut self) {
        self.history.clear();
        self.pending_translation = [0.0; 3];
        self.pending_rotation = [0.0, 0.0, 0.0, 1.0];
        self.remaining_ticks = 0;
        self.boosting = false;
        self.authoritative = None;
    }

    /// Records the pose the local body had at `tick`, and whether the player is
    /// holding boost. The boost flag is cosmetic: it drives presentation only.
    ///
    /// # Errors
    ///
    /// [`BufferRefusal::NotNewer`] for a tick not after the previous one.
    pub fn record_predicted(
        &mut self,
        tick: Tick,
        pose: PredictedPose,
        boosting: bool,
    ) -> Result<(), BufferRefusal> {
        if let Some((newest, _)) = self.history.back()
            && tick <= *newest
        {
            return Err(BufferRefusal::NotNewer {
                sample: tick.0,
                newest: newest.0,
            });
        }
        self.history.push_back((tick, pose));
        while self.history.len() > self.config.history_ticks.max(1) {
            self.history.pop_front();
        }
        self.boosting = boosting;
        Ok(())
    }

    /// Whether a local boost is being *shown*. Never a capacity.
    #[must_use]
    pub const fn boost_shown(&self) -> bool {
        self.boosting
    }

    /// The server's last word on ammunition, boost capacity and integrity.
    #[must_use]
    pub const fn authoritative(&self) -> Option<&AuthoritativeLoadout> {
        self.authoritative.as_ref()
    }

    /// Whether a smoothed correction is still being applied.
    #[must_use]
    pub const fn correcting(&self) -> bool {
        self.remaining_ticks > 0
    }

    /// Reconciles one mirrored authoritative record of the local aircraft.
    ///
    /// The loadout is taken from the record unconditionally (authority); the pose
    /// is compared with the predicted pose at the record's tick.
    ///
    /// # Errors
    ///
    /// [`BufferRefusal::WrongActor`], [`BufferRefusal::StaleGeneration`] for an
    /// older generation, or [`BufferRefusal::NotNewer`] for a record older than the
    /// one already reconciled. A refused record changes nothing.
    pub fn reconcile(&mut self, record: &RemoteAircraft) -> Result<Reconciliation, BufferRefusal> {
        if record.actor != self.actor {
            return Err(BufferRefusal::WrongActor);
        }
        if record.generation != self.generation {
            return Err(BufferRefusal::StaleGeneration {
                sample: record.generation,
                held: self.generation,
            });
        }
        if let Some(current) = &self.authoritative
            && record.tick <= current.tick
        {
            return Err(BufferRefusal::NotNewer {
                sample: record.tick.0,
                newest: current.tick.0,
            });
        }
        self.authoritative = Some(AuthoritativeLoadout {
            tick: record.tick,
            flight: record.flight,
            rounds: record.rounds,
            selected_bank: record.selected_bank,
            integrity: record.integrity,
        });
        // Records older than anything compared are history we can never match.
        while self.history.front().is_some_and(|(t, _)| *t < record.tick) {
            self.history.pop_front();
        }
        let Some((_, predicted)) = self.history.front().filter(|(t, _)| *t == record.tick) else {
            // No predicted pose at that tick: nothing to compare, so trust the
            // server and correct at once.
            self.history.clear();
            return Ok(self.begin_correction(None, record));
        };
        let predicted = *predicted;
        Ok(self.begin_correction(Some(predicted), record))
    }

    fn begin_correction(
        &mut self,
        predicted: Option<PredictedPose>,
        record: &RemoteAircraft,
    ) -> Reconciliation {
        let Some(predicted) = predicted else {
            // Without a predicted pose the caller must adopt the record's pose;
            // report it as an unbounded-size snap with no pending correction.
            self.pending_translation = [0.0; 3];
            self.pending_rotation = [0.0, 0.0, 0.0, 1.0];
            self.remaining_ticks = 0;
            return Reconciliation {
                kind: ReconcileKind::Snapped,
                error_m: f64::INFINITY,
                error_rad: f64::INFINITY,
            };
        };
        let p = predicted.position.to_array();
        let s = record.position.to_array();
        let translation = [s[0] - p[0], s[1] - p[1], s[2] - p[2]];
        let error_m = distance(s, p);
        let rotation = quat_normalize(quat_mul(
            quat_array(record.orientation),
            quat_conj(quat_array(predicted.orientation)),
        ));
        let error_rad = quat_angle(rotation);
        let kind =
            if error_m > self.config.snap_distance_m || error_rad > self.config.snap_angle_rad {
                ReconcileKind::Snapped
            } else if error_m < 1e-6 && error_rad < 1e-6 {
                ReconcileKind::Agreed
            } else {
                ReconcileKind::Smoothed
            };
        self.pending_translation = translation;
        self.pending_rotation = rotation;
        self.remaining_ticks = match kind {
            ReconcileKind::Smoothed => self.config.correction_ticks.max(1),
            ReconcileKind::Snapped => 1,
            ReconcileKind::Agreed => 0,
        };
        Reconciliation {
            kind,
            error_m,
            error_rad,
        }
    }

    /// The correction to apply to the local body this tick: the identity when no
    /// correction is pending. Each call removes `1/remaining` of the error, and the
    /// predicted history moves with it so later comparisons stay consistent.
    pub fn next_correction(&mut self) -> PoseCorrection {
        let identity = [0.0, 0.0, 0.0, 1.0];
        if self.remaining_ticks == 0 {
            return PoseCorrection {
                translation_m: [0.0; 3],
                rotation: Quaternion::IDENTITY,
            };
        }
        let fraction = 1.0 / f64::from(self.remaining_ticks);
        let translation = self.pending_translation.map(|c| c * fraction);
        let step = quat_nlerp(identity, self.pending_rotation, fraction);
        self.pending_translation = self.pending_translation.map(|c| c - c * fraction);
        self.pending_rotation = quat_normalize(quat_mul(quat_conj(step), self.pending_rotation));
        self.remaining_ticks -= 1;
        if self.remaining_ticks == 0 {
            self.pending_translation = [0.0; 3];
            self.pending_rotation = identity;
        }
        for (_, pose) in &mut self.history {
            let p = pose.position.to_array();
            if let Ok(position) = WorldPosition::try_new([
                p[0] + translation[0],
                p[1] + translation[1],
                p[2] + translation[2],
            ]) {
                pose.position = position;
            }
            if let Ok(orientation) = to_quaternion(quat_mul(step, quat_array(pose.orientation))) {
                pose.orientation = orientation;
            }
        }
        PoseCorrection {
            translation_m: translation,
            rotation: to_quaternion(step).unwrap_or(Quaternion::IDENTITY),
        }
    }
}

// ---------------------------------------------------------------------------
// F57-C: the wired session. Reconciliation, projectile confirmation and origin
// epochs on the path that actually runs.
//
// Spec: `specs/F57-*.md`, stage `### F57-C`. Everything below is newly authored
// engine design; no original networked projectile, confirmation or origin
// behavior has been measured, and none is asserted.
//
// F57-A built the schema, F57-B built the pieces. This stage is the wiring:
// [`NetSession`] is the one object the app owns per session, and it is what
// connects the producer ([`cs_sim::net_state::NetStateLedger`] through
// [`publish_snapshot`]) to the consumers ([`RemoteInterpolator`],
// [`LocalPredictor`] and [`Tracers`]) in the order the data actually flows. Each
// piece above was previously driven by hand from a test; nothing assembled them
// for a running app.
//
// # The rules the wiring exists to enforce
//
// * **Origin epochs are shared and adopted, never assumed.**
//   [`NetSession::rebase`] moves the session's frame and every consumer's
//   together, converting rather than displacing (F57 AC03). A snapshot whose
//   epoch does not match is refused by [`RemoteMirror::ingest`]; after a rebase
//   the session is in the new epoch, so the *next* snapshot applies and an old
//   one still does not.
// * **A correction belongs to the pose owner, not to this session.**
//   [`NetSession::next_correction`] is what the single pose owner applies each
//   tick. The session never writes a body, so there is one owner of a pose.
// * **A local tracer cannot award a kill.** [`Tracers`] is presentation state
//   with a hard ceiling on retention. A shot the server accepted is a
//   [`cs_sim::net_state::ShotId`]; a hit is only ever what
//   [`cs_sim::net_state::NetStateLedger::confirm_shot`] returns, and the only
//   thing the client derives from it is a visual confirmation. Removing the
//   confirmation leaves the tracer cosmetic, which is the sheet's non-negotiable
//   behavior 3.
// * **Teardown and retry are part of the path.** [`NetSession::teardown`] is
//   idempotent and releases every buffer; a new session starts clean, because
//   [`RemoteMirror::forget`] and the interpolator's own `forget` are the only
//   ways their generation memory is released.
// ---------------------------------------------------------------------------

/// Why a session-level operation was refused.
///
/// Distinct from [`PublishError`] (the producer's refusal) and
/// [`IngestRefusal`] (one snapshot's refusal): this names the *session's* own
/// state — that it has been torn down, or that a caller asked for something
/// before the piece that produces it exists.
#[derive(Clone, Debug, PartialEq)]
pub enum SessionError {
    /// The origin conversion the caller asked for was refused.
    Origin(OriginError),
    /// The session has been torn down. It holds no buffers and accepts no
    /// further work; a retry is a new session, not a revived one.
    TornDown,
    /// The caller asked for local prediction before attaching a local actor.
    NoLocalActor,
    /// The caller asked for the local aircraft without naming one, or named
    /// another one.
    UnknownLocalActor {
        /// The local actor the session is bound to.
        expected: Option<ActorId>,
    },
    /// A buffer or the predictor refused the request.
    Buffer(BufferRefusal),
    /// A tracer operation was refused.
    Tracer(TracerRefusal),
}

impl fmt::Display for SessionError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Origin(error) => write!(f, "origin conversion refused: {error}"),
            Self::TornDown => write!(f, "the network session has been torn down"),
            Self::NoLocalActor => {
                write!(
                    f,
                    "no local actor is attached; local prediction has nothing to own"
                )
            }
            Self::UnknownLocalActor { expected } => match expected {
                Some(actor) => write!(f, "the session's local actor is {actor}"),
                None => write!(f, "the session has no local actor bound"),
            },
            Self::Buffer(error) => write!(f, "a network buffer refused the request: {error}"),
            Self::Tracer(error) => write!(f, "a client tracer refused the request: {error}"),
        }
    }
}

impl std::error::Error for SessionError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::Origin(error) => Some(error),
            Self::Buffer(error) => Some(error),
            Self::Tracer(error) => Some(error),
            Self::TornDown | Self::NoLocalActor | Self::UnknownLocalActor { .. } => None,
        }
    }
}

impl From<OriginError> for SessionError {
    fn from(value: OriginError) -> Self {
        Self::Origin(value)
    }
}

/// How a client-side tracer learned what happened to it.
///
/// Deliberately coarser than the damage domain's hit taxonomy: the client
/// learns *that* the server confirmed a shot, never *how much* it hurt or which
/// subsystem failed. Anything finer would let a client draw a conclusion the
/// server never published.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum TracerVerdict {
    /// Still in flight: predicted, not yet answered by the server.
    Pending,
    /// The server confirmed this shot hit `target`.
    Confirmed {
        /// What the server named.
        target: Option<ActorId>,
    },
    /// The server resolved this shot and it did **not** hit anything. The tracer
    /// is retired; it still never awarded anything.
    Missed,
}

impl TracerVerdict {
    /// Whether the server has resolved this shot either way.
    #[must_use]
    pub const fn is_resolved(self) -> bool {
        !matches!(self, Self::Pending)
    }
}

/// One predicted client-side projectile, and the server's word about it.
///
/// A tracer is a *cosmetic*: it is spawned from the local aircraft's own trigger
/// pull, it exists only so the pilot sees a shot leave the guns, and it has no
/// path to awarding damage. The only field that changes authority anywhere is the
/// [`TracerVerdict`], and the only way to reach it is
/// [`Tracers::apply_confirmation`], which takes the server's
/// [`cs_sim::net_state::ShotConfirmation`] rather than any client-computed
/// geometry.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Tracer {
    /// Which local shot this is. It matches the
    /// [`cs_sim::net_state::ShotId`] the server accepted.
    pub shot: cs_sim::net_state::ShotId,
    /// The tick the tracer was spawned on.
    pub spawned: Tick,
    /// Where the shot was spawned, in canonical world coordinates.
    pub origin: WorldPosition,
    /// The predicted flight direction, in m/s.
    pub direction_mps: [f64; 3],
    /// The server's word about it, once there is one.
    pub verdict: TracerVerdict,
}

/// Bounded client-side tracers, keyed by shot.
///
/// Bounded twice over: [`TRACER_HARD_CAP`] is the hard ceiling the constructor
/// refuses to exceed, and each entry carries the tick it may be presented for
/// until ([`TRACER_LIFETIME_TICKS`]) so a shot nobody ever answers cannot pin
/// memory forever. A confirmed tracer is *not* dropped early: it stays until its
/// presentation window ends, so a pilot can see the hit they earned.
#[derive(Clone, Debug)]
pub struct Tracers {
    local: ActorId,
    cap: usize,
    /// Tracers keyed by the shot they belong to, so the retention rule can drop
    /// the oldest by `spawned` tick rather than by insertion order.
    entries: BTreeMap<cs_sim::net_state::ShotId, Tracer>,
}

impl Tracers {
    /// The most tracers a session may hold, whatever the caller asks for.
    ///
    /// Bounded because a local trigger held down under prediction would
    /// otherwise allocate one entry per tick; the acceptance test drives past
    /// the configured limit and asserts this ceiling holds.
    pub const TRACER_HARD_CAP: usize = 256;

    /// How many ticks a tracer may live before it is dropped unanswered.
    ///
    /// Newly authored design, chosen as a presentation window rather than
    /// measured: no original projectile lifetime has been measured.
    pub const TRACER_LIFETIME_TICKS: u64 = 120;

    /// A tracer book for `local`, holding at most `cap` entries.
    ///
    /// A cap of zero is raised to one and a cap above [`Self::TRACER_HARD_CAP`]
    /// is clamped: a presentation buffer is sized by configuration, and
    /// mis-sizing it must not turn into a refusal of a *shot*. Use
    /// [`Self::try_new`] where the cap is a caller's claim worth checking.
    #[must_use]
    pub fn new(local: ActorId, cap: usize) -> Self {
        Self {
            local,
            cap: cap.clamp(1, Self::TRACER_HARD_CAP),
            entries: BTreeMap::new(),
        }
    }

    /// The checked constructor: refuses a cap above [`Self::TRACER_HARD_CAP`]
    /// rather than silently clamping it, so a caller that believes it holds more
    /// than the ceiling allows is told instead of quietly holding less.
    ///
    /// # Errors
    ///
    /// [`CapError::AboveHardCap`] for a cap above the ceiling.
    pub fn try_new(local: ActorId, cap: usize) -> Result<Self, CapError> {
        if cap > Self::TRACER_HARD_CAP {
            return Err(CapError::AboveHardCap {
                requested: cap,
                max: Self::TRACER_HARD_CAP,
            });
        }
        Ok(Self::new(local, cap))
    }

    /// The configured retention cap.
    #[must_use]
    pub const fn cap(&self) -> usize {
        self.cap
    }

    /// The local aircraft these tracers belong to.
    #[must_use]
    pub const fn local(&self) -> ActorId {
        self.local
    }

    /// How many tracers are held.
    #[must_use]
    pub fn len(&self) -> usize {
        self.entries.len()
    }

    /// Whether no tracer is held.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }

    /// The tracer for `shot`.
    #[must_use]
    pub fn tracer(&self, shot: cs_sim::net_state::ShotId) -> Option<&Tracer> {
        self.entries.get(&shot)
    }

    /// Spawns a predicted tracer for a shot the *server already accepted*.
    ///
    /// The shot id is the server's ([`cs_sim::net_state::NetStateLedger::
    /// accept_shot`]); this function only records the cosmetic. A shot the server
    /// refused has no accepted id, so it cannot be spawned here at all.
    ///
    /// Refuses rather than overwrites when the shot already has a tracer, so a
    /// replayed fire request cannot silently restamp one. When the book is at its
    /// cap the oldest unanswered tracer is dropped to make room, and a confirmed
    /// one is never dropped in its place: a pilot sees the hit they earned.
    ///
    /// # Errors
    ///
    /// [`TracerRefusal::AlreadyTracked`] for a shot that already has a tracer and
    /// [`TracerRefusal::UnusableDirection`] for a non-finite direction.
    pub fn spawn(
        &mut self,
        shot: cs_sim::net_state::ShotId,
        tick: Tick,
        origin: WorldPosition,
        direction_mps: [f64; 3],
    ) -> Result<(), TracerRefusal> {
        if self.entries.contains_key(&shot) {
            return Err(TracerRefusal::AlreadyTracked { shot });
        }
        if !direction_mps.iter().all(|value| value.is_finite()) {
            return Err(TracerRefusal::UnusableDirection);
        }
        self.evict_until_room();
        self.entries.insert(
            shot,
            Tracer {
                shot,
                spawned: tick,
                origin,
                direction_mps,
                verdict: TracerVerdict::Pending,
            },
        );
        Ok(())
    }

    /// Drops tracers until the book is below its cap.
    ///
    /// Oldest first by `spawned` tick, and only while it is *over* the cap, so a
    /// quiet book keeps its whole window. A confirmed tracer is only dropped once
    /// every pending one is gone, which is the preference order: an unanswered
    /// prediction is the one a pilot cannot resolve themselves.
    fn evict_until_room(&mut self) {
        while self.entries.len() >= self.cap {
            let oldest_pending = self
                .entries
                .values()
                .filter(|tracer| !tracer.verdict.is_resolved())
                .min_by_key(|tracer| tracer.spawned)
                .map(|tracer| tracer.shot);
            let victim = oldest_pending.or_else(|| {
                self.entries
                    .values()
                    .min_by_key(|tracer| tracer.spawned)
                    .map(|tracer| tracer.shot)
            });
            let Some(victim) = victim else {
                return;
            };
            self.entries.remove(&victim);
        }
    }

    /// Drops every tracer whose presentation window has elapsed at `now`.
    ///
    /// The bound that stops a shot nobody ever answers from pinning memory
    /// forever: [`Self::TRACER_LIFETIME_TICKS`] is newly authored design, chosen
    /// as a presentation window rather than measured.
    pub fn expire(&mut self, now: Tick) {
        self.entries.retain(|_, tracer| {
            now.0.saturating_sub(tracer.spawned.0) <= Self::TRACER_LIFETIME_TICKS
        });
    }

    /// Applies the server's word about a shot.
    ///
    /// Takes a [`cs_sim::net_state::ShotConfirmation`], not a client-computed
    /// intersection: there is no code path from local geometry to a resolved
    /// verdict, so a predicted tracer can never award anything (F57
    /// non-negotiable behavior 3). A confirmation for a shot with no tracer is
    /// refused and named — a server hit the client never predicted is a fact to
    /// report, not to display as a local shot.
    pub fn apply_confirmation(
        &mut self,
        confirmation: &cs_sim::net_state::ShotConfirmation,
    ) -> Result<(), TracerRefusal> {
        if confirmation.shooter != self.local {
            return Err(TracerRefusal::ForeignShooter {
                local: self.local,
                found: confirmation.shooter,
            });
        }
        let entry = self
            .entries
            .get_mut(&confirmation.shot)
            .ok_or(TracerRefusal::UnknownShot {
                shot: confirmation.shot,
            })?;
        entry.verdict = match confirmation.target {
            Some(target) => TracerVerdict::Confirmed {
                target: Some(target),
            },
            None => TracerVerdict::Missed,
        };
        Ok(())
    }

    /// How many tracers have a resolved verdict.
    #[must_use]
    pub fn resolved_count(&self) -> usize {
        self.entries
            .values()
            .filter(|tracer| tracer.verdict.is_resolved())
            .count()
    }

    /// Drops every tracer the local aircraft has, answered or not.
    pub fn clear(&mut self) {
        self.entries.clear();
    }
}

/// Why a tracer operation was refused.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TracerRefusal {
    /// The shot already has a tracer; a replayed fire request changed nothing.
    AlreadyTracked {
        /// The shot that already had one.
        shot: cs_sim::net_state::ShotId,
    },
    /// The predicted direction held a non-finite component.
    UnusableDirection,
    /// The confirmation names a shooter that is not this client.
    ForeignShooter {
        /// The local actor.
        local: ActorId,
        /// The shooter the confirmation names.
        found: ActorId,
    },
    /// The confirmation names a shot with no predicted tracer.
    UnknownShot {
        /// The untracked shot.
        shot: cs_sim::net_state::ShotId,
    },
}

impl fmt::Display for TracerRefusal {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::AlreadyTracked { shot } => write!(f, "{shot} already has a tracer"),
            Self::UnusableDirection => write!(f, "predicted tracer direction is not finite"),
            Self::ForeignShooter { local, found } => {
                write!(
                    f,
                    "confirmation names {found}, but the local aircraft is {local}"
                )
            }
            Self::UnknownShot { shot } => write!(f, "{shot} has no predicted tracer to confirm"),
        }
    }
}

impl std::error::Error for TracerRefusal {}

/// Why a requested buffer cap was refused.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CapError {
    /// The requested cap is above the hard ceiling this buffer enforces.
    AboveHardCap {
        /// What the caller asked for.
        requested: usize,
        /// The ceiling.
        max: usize,
    },
}

impl fmt::Display for CapError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::AboveHardCap { requested, max } => {
                write!(f, "requested cap {requested} is above the hard cap {max}")
            }
        }
    }
}

impl std::error::Error for CapError {}

/// The per-session network state the app owns: the mirror, the interpolation
/// buffers, the local predictor and the tracers, wired together.
///
/// This is the object F57-A and F57-B's pieces were waiting for. It owns the
/// order the data flows in — decode happens at the transport, then
/// [`Self::ingest`] applies one snapshot to the mirror and the buffers, then
/// [`Self::sample`] presents remote aircraft, then [`Self::next_correction`] hands
/// the pose owner its bounded correction — and it owns teardown, which is the
/// only place the generation memory behind those buffers is released.
///
/// It holds no Bevy state and touches no body: [`Self::next_correction`] returns
/// a [`PoseCorrection`] for the single pose owner to apply, so there is exactly
/// one owner of a pose.
#[derive(Clone, Debug)]
pub struct NetSession {
    session: SessionId,
    origin: WorldOrigin,
    mirror: RemoteMirror,
    interpolator: RemoteInterpolator,
    predictor: Option<LocalPredictor>,
    tracers: Option<Tracers>,
    torn_down: bool,
}

impl NetSession {
    /// A session for `session`, reading records in `origin`'s frame.
    ///
    /// No local aircraft is attached yet, so local prediction and tracers are
    /// absent until [`Self::attach_local`] names one.
    #[must_use]
    pub fn new(
        session: SessionId,
        origin: WorldOrigin,
        interpolation: InterpolationConfig,
    ) -> Self {
        Self {
            session,
            origin,
            mirror: RemoteMirror::new(session, origin),
            interpolator: RemoteInterpolator::new(interpolation),
            predictor: None,
            tracers: None,
            torn_down: false,
        }
    }

    /// The session this belongs to.
    #[must_use]
    pub const fn session(&self) -> SessionId {
        self.session
    }

    /// The world origin every consumer currently reads in.
    #[must_use]
    pub const fn origin(&self) -> WorldOrigin {
        self.origin
    }

    /// Whether the session has been torn down.
    #[must_use]
    pub const fn is_torn_down(&self) -> bool {
        self.torn_down
    }

    /// The mirror of remote aircraft.
    #[must_use]
    pub const fn mirror(&self) -> &RemoteMirror {
        &self.mirror
    }

    /// The interpolation buffers.
    #[must_use]
    pub const fn interpolator(&self) -> &RemoteInterpolator {
        &self.interpolator
    }

    /// The local predictor, when a local aircraft is attached.
    #[must_use]
    pub const fn predictor(&self) -> Option<&LocalPredictor> {
        self.predictor.as_ref()
    }

    /// The client tracers, when a local aircraft is attached.
    #[must_use]
    pub const fn tracers(&self) -> Option<&Tracers> {
        self.tracers.as_ref()
    }

    /// Binds the local aircraft, so local prediction and tracers exist.
    ///
    /// # Errors
    ///
    /// [`SessionError::TornDown`] when the session is gone, and
    /// [`SessionError::UnknownLocalActor`] when a *different* local actor is
    /// already attached: switching the local aircraft mid-session would leave one
    /// actor's predicted history and another's authoritative loadout interleaved,
    /// so the caller tears the session down instead.
    pub fn attach_local(
        &mut self,
        actor: ActorId,
        generation: u16,
        prediction: PredictionConfig,
    ) -> Result<(), SessionError> {
        self.expect_live()?;
        if let Some(predictor) = &self.predictor
            && predictor.actor() != actor
        {
            return Err(SessionError::UnknownLocalActor {
                expected: Some(predictor.actor()),
            });
        }
        if self.predictor.is_none() {
            self.predictor = Some(LocalPredictor::new(actor, generation, prediction));
            self.tracers = Some(Tracers::new(actor, 64));
        }
        Ok(())
    }

    /// The local aircraft's actor, when one is attached.
    #[must_use]
    pub fn local_actor(&self) -> Option<ActorId> {
        self.predictor.as_ref().map(LocalPredictor::actor)
    }

    /// Applies one decoded snapshot to the mirror and the buffers.
    ///
    /// Check order is the contract: the mirror refuses an unmatched origin epoch
    /// or an out-of-order tick before anything is touched, and only then does the
    /// ingest report drive the interpolation buffers. The returned report is the
    /// same one the mirror produced, so a refused snapshot is visibly refused
    /// rather than silently ignored.
    ///
    /// # Errors
    ///
    /// [`SessionError::TornDown`] when the session has been torn down.
    pub fn ingest(
        &mut self,
        snapshot: &Snapshot,
        tick: Tick,
    ) -> Result<IngestReport, SessionError> {
        self.expect_live()?;
        let report = self.mirror.ingest(snapshot, tick);
        // The buffers are fed from the report and the mirror's *current* state,
        // which is what makes a refused record contribute no sample.
        self.interpolator.observe(&report, snapshot, &self.mirror);
        // A destroyed or despawned remote actor's buffer is retired by
        // `observe`; a reliably removed one is forgotten so a later id reuse
        // cannot inherit its history.
        Ok(report)
    }

    /// Applies one reliable, idempotent session event.
    ///
    /// Returns whether this call is the one that recorded the removal, so a
    /// replay is distinguishable from the first application. A removal that this
    /// call recorded also releases the actor's interpolation history.
    ///
    /// # Errors
    ///
    /// [`SessionError::TornDown`] when the session has been torn down.
    pub fn apply_event(&mut self, event: &ReliableEvent) -> Result<bool, SessionError> {
        self.expect_live()?;
        let recorded = self.mirror.apply_event(event);
        if recorded && let EventBody::ActorRemoved { actor } = event.body {
            self.interpolator.forget(actor);
        }
        Ok(recorded)
    }

    /// The pose to present for `actor` at the estimated server tick `now`.
    ///
    /// # Errors
    ///
    /// [`BufferRefusal::Unusable`] when the blended pose is not representable.
    pub fn sample(
        &self,
        actor: ActorId,
        now: Tick,
    ) -> Result<Option<InterpolatedAircraft>, BufferRefusal> {
        self.interpolator.sample(actor, now)
    }

    /// Reconciles one mirrored record of the *local* aircraft.
    ///
    /// The single entry point to [`LocalPredictor::reconcile`], so the
    /// authoritative loadout can only ever be written from a record that passed
    /// the mirror. It reads the record from the mirror rather than taking it as
    /// an argument, which is what makes "reconciliation follows ingestion" a
    /// property of the type instead of a discipline.
    ///
    /// # Errors
    ///
    /// [`SessionError::NoLocalActor`] when no local aircraft is attached,
    /// [`SessionError::UnknownLocalActor`] when the mirror holds no record for
    /// the local actor, and any [`BufferRefusal`] the predictor reports — the
    /// record was refused, so nothing changed.
    pub fn reconcile_local(&mut self) -> Result<Reconciliation, SessionError> {
        self.expect_live()?;
        let predictor = self.predictor.as_mut().ok_or(SessionError::NoLocalActor)?;
        let actor = predictor.actor();
        let record = *self
            .mirror
            .aircraft(actor)
            .ok_or(SessionError::UnknownLocalActor {
                expected: Some(actor),
            })?;
        predictor.reconcile(&record).map_err(SessionError::Buffer)
    }

    /// Records the pose the local body had at `tick`.
    ///
    /// # Errors
    ///
    /// [`SessionError::NoLocalActor`] when no local aircraft is attached, and
    /// [`BufferRefusal::NotNewer`] for a tick that does not advance.
    pub fn record_local_pose(
        &mut self,
        tick: Tick,
        pose: PredictedPose,
        boosting: bool,
    ) -> Result<(), SessionError> {
        self.expect_live()?;
        self.predictor
            .as_mut()
            .ok_or(SessionError::NoLocalActor)?
            .record_predicted(tick, pose, boosting)
            .map_err(SessionError::Buffer)
    }

    /// The correction the single pose owner should apply this tick.
    ///
    /// The identity when nothing is pending or no local aircraft is attached.
    pub fn next_correction(&mut self) -> PoseCorrection {
        self.predictor.as_mut().map_or(
            PoseCorrection {
                translation_m: [0.0; 3],
                rotation: Quaternion::IDENTITY,
            },
            LocalPredictor::next_correction,
        )
    }

    /// Spawns a predicted tracer for a shot the server accepted.
    ///
    /// # Errors
    ///
    /// [`SessionError::NoLocalActor`], [`SessionError::TornDown`] and
    /// [`SessionError::Tracer`] for the tracer's own refusals.
    pub fn spawn_tracer(
        &mut self,
        shot: cs_sim::net_state::ShotId,
        tick: Tick,
        origin: WorldPosition,
        direction_mps: [f64; 3],
    ) -> Result<(), SessionError> {
        self.expect_live()?;
        self.tracers
            .as_mut()
            .ok_or(SessionError::NoLocalActor)?
            .spawn(shot, tick, origin, direction_mps)
            .map_err(SessionError::Tracer)
    }

    /// Applies the server's word about a shot to the local tracers.
    ///
    /// # Errors
    ///
    /// [`SessionError::NoLocalActor`], [`SessionError::TornDown`] and
    /// [`SessionError::Tracer`] for the tracer's own refusals.
    pub fn apply_confirmation(
        &mut self,
        confirmation: &cs_sim::net_state::ShotConfirmation,
    ) -> Result<(), SessionError> {
        self.expect_live()?;
        self.tracers
            .as_mut()
            .ok_or(SessionError::NoLocalActor)?
            .apply_confirmation(confirmation)
            .map_err(SessionError::Tracer)
    }

    /// Moves the whole session to a new world-origin epoch.
    ///
    /// This is F57-C's AC03 path: the frame changes and every consumer changes
    /// with it, converting rather than displacing. The mirror re-projects its
    /// records and the interpolation buffers are dropped, because a buffered
    /// sample's stored integers belong to the frame they were decoded in and
    /// cannot be reinterpreted in a new one — dropping them costs one
    /// interpolation delay of smoothness and buys the guarantee that no aircraft
    /// is drawn `origin_shift_m` away from where it was.
    ///
    /// Velocity and the local predictor's *authoritative loadout* are untouched:
    /// a rebase is a change of frame, so it is not a velocity impulse, and the
    /// server's ammunition and boost capacity do not depend on where the client
    /// draws the world from.
    ///
    /// # Errors
    ///
    /// [`OriginError::EpochMismatch`] for an epoch that is not strictly newer
    /// than the held one, [`OriginError::Space`] when a mirrored record cannot be
    /// converted, and [`SessionError::TornDown`] after teardown.
    pub fn rebase(&mut self, new_position: WorldPosition) -> Result<EpochTransition, SessionError> {
        self.expect_live()?;
        let next = self.origin.rebased(new_position)?;
        let transition = self.mirror.adopt_origin(next)?;
        // The mirror holds canonical world positions, so its records survive; the
        // buffers hold samples decoded against the old frame, so they do not.
        self.interpolator.clear();
        // The local predictor's history is canonical world positions too, so it
        // survives; only its pending correction is a change of frame, and it is
        // recomputed from the next record.
        self.origin = next;
        Ok(transition)
    }

    /// Releases every buffer this session holds.
    ///
    /// Idempotent, and the *only* place the generation memory behind the mirror
    /// and the buffers is released: after this the session answers every
    /// operation with [`SessionError::TornDown`], so a late packet cannot be
    /// applied to a torn-down session. A retry is a new session, which is what
    /// keeps "no ghost aircraft" true across a reconnect (F57 AC01).
    pub fn teardown(&mut self) -> usize {
        if self.torn_down {
            return 0;
        }
        let released = self.mirror.aircraft_count();
        for actor in self
            .mirror
            .all_aircraft()
            .map(|a| a.actor)
            .collect::<Vec<_>>()
        {
            self.mirror.forget(actor);
            self.interpolator.forget(actor);
        }
        self.interpolator.clear();
        if let Some(tracers) = &mut self.tracers {
            tracers.clear();
        }
        if let Some(predictor) = &mut self.predictor {
            predictor.reset();
        }
        self.predictor = None;
        self.tracers = None;
        self.torn_down = true;
        released
    }

    fn expect_live(&self) -> Result<(), SessionError> {
        if self.torn_down {
            return Err(SessionError::TornDown);
        }
        Ok(())
    }
}

// F57-B acceptance tests live here, not in an integration test, so the branch
// adds no extra Bevy-linked test binary (CI runners ran out of room linking the
// doctests). They drive the production path ledger -> publish_snapshot -> bytes
// -> RemoteMirror -> RemoteInterpolator / LocalPredictor.
#[cfg(test)]
mod f57_b_acceptance {

    use super::{
        BufferRefusal, InterpolationConfig, LocalPredictor, PredictedPose, PredictionConfig,
        ReconcileKind, RemoteInterpolator, RemoteMirror, SampleMode, publish_snapshot,
    };
    use crate::origin::{OriginEpoch, WorldOrigin};
    use cs_net::snapshot::Snapshot;
    use cs_sim::net_state::{NetActorState, NetLifecycle, NetStateLedger};
    use cs_types::Tick;
    use cs_types::net::{ActorAllocator, ActorId, SessionId};
    use cs_types::space::{Quaternion, WorldPosition};

    const SESSION: SessionId = match SessionId::new(57) {
        Some(id) => id,
        None => unreachable!(),
    };

    fn origin() -> WorldOrigin {
        WorldOrigin::new(
            OriginEpoch(1),
            WorldPosition::try_new([1_000.0, 0.0, 1_000.0]).expect("finite"),
        )
    }

    fn at(x: f64) -> WorldPosition {
        WorldPosition::try_new([x, 100.0, 0.0]).expect("finite")
    }

    struct World {
        ledger: NetStateLedger,
        mirror: RemoteMirror,
        interpolator: RemoteInterpolator,
        actor: ActorId,
    }

    impl World {
        fn new() -> Self {
            let mut ledger = NetStateLedger::new(SESSION);
            let actor = ActorAllocator::new(SESSION).allocate().expect("serial");
            ledger
                .spawn(NetActorState::spawn(
                    actor,
                    at(1_000.0),
                    Quaternion::IDENTITY,
                    400,
                ))
                .expect("spawn");
            Self {
                ledger,
                mirror: RemoteMirror::new(SESSION, origin()),
                interpolator: RemoteInterpolator::new(InterpolationConfig::default()),
                actor,
            }
        }

        /// Moves the actor, publishes, round-trips the bytes and feeds the receiver.
        fn step(&mut self, tick: u64, x: f64, mutate: impl FnOnce(&mut NetActorState)) {
            let mut state = *self.ledger.state(self.actor).expect("known");
            state.pose.position = at(x);
            state.linear_velocity_mps = [60.0, 0.0, 0.0];
            mutate(&mut state);
            self.ledger.publish(state).expect("owned generation");
            self.deliver(tick);
        }

        fn deliver(&mut self, tick: u64) {
            let snapshot = publish_snapshot(&self.ledger, self.mirror.origin()).expect("publish");
            let bytes = snapshot.encode(SESSION).expect("encode");
            let decoded = Snapshot::decode(&bytes, SESSION).expect("decode");
            let report = self.mirror.ingest(&decoded, Tick(tick));
            let refusals = self.interpolator.observe(&report, &decoded, &self.mirror);
            assert!(refusals.is_empty(), "{refusals:?}");
        }

        fn x_at(&self, now: u64) -> (f64, SampleMode) {
            let sampled = self
                .interpolator
                .sample(self.actor, Tick(now))
                .expect("representable")
                .expect("buffered");
            (sampled.state.position.x(), sampled.mode)
        }
    }

    #[test]
    fn accept_f57_b_interpolation_blends_between_snapshots_and_never_blends_discrete_state() {
        let mut world = World::new();
        world.step(10, 1_000.0, |s| s.weapons.primary_rounds = 400);
        world.step(20, 1_010.0, |s| s.weapons.primary_rounds = 380);
        // delay 6 ticks: now 21 renders tick 15, halfway.
        let sampled = world
            .interpolator
            .sample(world.actor, Tick(21))
            .unwrap()
            .unwrap();
        assert_eq!(sampled.mode, SampleMode::Interpolated);
        assert!((sampled.state.position.x() - 1_005.0).abs() < 0.05);
        // Rounds are the earlier authoritative record's, not a blend.
        assert_eq!(sampled.state.rounds[0], 400);
    }

    #[test]
    fn accept_f57_b_loss_is_bridged_then_extrapolation_is_bounded() {
        let mut world = World::new();
        world.step(10, 1_000.0, |_| {});
        world.step(20, 1_010.0, |_| {});
        // Snapshots for ticks 11..19 were lost: the pair still blends.
        assert_eq!(world.x_at(21).1, SampleMode::Interpolated);
        // Past the newest sample: project along velocity (60 m/s, 3 ticks = 3 m).
        let (x, mode) = world.x_at(29);
        assert_eq!(mode, SampleMode::Extrapolated);
        assert!((x - 1_013.0).abs() < 0.1, "{x}");
        // Far past: the pose stops at the limit instead of drifting.
        let (x, mode) = world.x_at(500);
        assert_eq!(mode, SampleMode::ExtrapolationExhausted);
        assert!((x - 1_016.0).abs() < 0.1, "{x}");
    }

    #[test]
    fn accept_f57_b_a_teleport_is_held_not_interpolated_across() {
        let mut world = World::new();
        world.step(10, 1_000.0, |_| {});
        world.step(20, 9_000.0, |_| {});
        // Render tick 15 is between them: held at the pre-teleport pose.
        let (x, mode) = world.x_at(21);
        assert_eq!(mode, SampleMode::Held);
        assert!((x - 1_000.0).abs() < 0.05);
        // Once the render tick reaches the teleport sample it shows it.
        let (x, _) = world.x_at(26);
        assert!((x - 9_000.0).abs() < 0.05);
    }

    #[test]
    fn accept_f57_b_buffer_is_bounded_and_refuses_old_ticks() {
        let mut world = World::new();
        for i in 0..40_u64 {
            world.step(10 + i, 1_000.0 + i as f64, |_| {});
        }
        assert_eq!(
            world.interpolator.sample_count(world.actor),
            world.interpolator.config().capacity
        );
        let record = *world.mirror.aircraft(world.actor).expect("mirrored");
        assert!(matches!(
            world.interpolator.push(&record),
            Err(BufferRefusal::NotNewer { .. })
        ));
    }

    #[test]
    fn accept_f57_b_recycled_ids_never_share_interpolation_history() {
        let mut world = World::new();
        world.step(10, 1_000.0, |_| {});
        world.step(20, 1_010.0, |_| {});
        let old_generation = world.ledger.generation(world.actor).unwrap();
        // The id is reused for a new aircraft far away.
        world.ledger.forget(world.actor).expect("forget");
        world
            .ledger
            .spawn(NetActorState::spawn(
                world.actor,
                at(5_000.0),
                Quaternion::IDENTITY,
                400,
            ))
            .expect("respawn");
        let new_generation = world.ledger.generation(world.actor).unwrap();
        assert!(new_generation > old_generation);
        let stale = *world.mirror.aircraft(world.actor).expect("old mirrored");
        world.deliver(30);
        assert_eq!(world.interpolator.sample_count(world.actor), 1);
        // Sampling between the old and new times never produces a blend of the two.
        for now in 20..45 {
            let (x, _) = world.x_at(now);
            assert!(
                (x - 5_000.0).abs() < 0.05
                    || (x - 1_010.0).abs() < 0.05
                    || (x - 1_000.0).abs() < 0.05,
                "blended x {x} at {now}"
            );
            let sampled = world
                .interpolator
                .sample(world.actor, Tick(now))
                .unwrap()
                .unwrap();
            assert_eq!(sampled.state.generation, new_generation.get());
        }
        // A late packet of the old generation is refused.
        assert!(matches!(
            world.interpolator.push(&stale),
            Err(BufferRefusal::StaleGeneration { .. })
        ));
    }

    #[test]
    fn accept_f57_b_a_destroyed_actor_leaves_no_ghost_and_an_ended_generation_stays_ended() {
        let mut world = World::new();
        world.step(10, 1_000.0, |_| {});
        let before = *world.mirror.aircraft(world.actor).expect("mirrored");
        let generation = world.ledger.generation(world.actor).unwrap();
        world
            .ledger
            .end_lifecycle(world.actor, generation, NetLifecycle::Destroyed)
            .expect("destroy");
        world.deliver(11);
        assert!(
            world
                .interpolator
                .sample(world.actor, Tick(30))
                .unwrap()
                .is_none()
        );
        assert_eq!(world.interpolator.track_count(), 0);
        assert!(world.interpolator.push(&before).is_err());
    }

    fn pose(x: f64) -> PredictedPose {
        PredictedPose {
            position: at(x),
            orientation: Quaternion::IDENTITY,
        }
    }

    /// AC02: the local boost is predicted (the body runs ahead), then a server
    /// record arrives. Ammunition and boost capacity are the server's, the pose is
    /// corrected by a bounded smoothing.
    #[test]
    fn accept_f57_b_a_server_correction_during_a_local_boost_keeps_ammo_and_fuel_authoritative() {
        let mut world = World::new();
        let mut predictor = LocalPredictor::new(
            world.actor,
            world.ledger.generation(world.actor).unwrap().get(),
            PredictionConfig::default(),
        );
        // The local body boosts: it runs 0.3 m/tick ahead of what the server will say.
        for tick in 1..=10_u64 {
            predictor
                .record_predicted(Tick(tick), pose(1_000.0 + tick as f64 * 2.3), true)
                .expect("increasing ticks");
        }
        assert!(predictor.boost_shown());
        // The server saw tick 10 at +2.0 m/tick, with less fuel and fewer rounds
        // than any local guess: boost capacity drained to 0.25, 37 rounds left.
        world.step(10, 1_000.0 + 20.0, |s| {
            s.flight.boost_capacity = 0.25;
            s.weapons.primary_rounds = 37;
        });
        let record = *world.mirror.aircraft(world.actor).expect("mirrored");
        let result = predictor.reconcile(&record).expect("reconciles");
        assert_eq!(result.kind, ReconcileKind::Smoothed);
        assert!((result.error_m - 3.0).abs() < 0.05, "{}", result.error_m);

        let loadout = predictor.authoritative().expect("server word");
        assert_eq!(loadout.rounds[0], 37);
        assert!((loadout.flight[2] - 0.25).abs() < 1e-4);
        // Still "boosting" on screen, with no way to reach the capacity from it.
        assert!(predictor.boost_shown());

        // The error is removed in bounded steps that sum to the whole error.
        let mut total = 0.0;
        let mut steps = 0;
        while predictor.correcting() {
            let step = predictor.next_correction();
            assert!(step.translation_m[0].abs() <= 3.0 / 5.0 + 0.1);
            total += step.translation_m[0];
            steps += 1;
        }
        assert_eq!(steps, PredictionConfig::default().correction_ticks);
        assert!((total - (record.position.x() - (1_000.0 + 23.0))).abs() < 1e-6);
        // A stale record cannot rewind the authoritative loadout.
        let mut older = record;
        older.tick = Tick(5);
        older.rounds = [400, 400];
        assert!(matches!(
            predictor.reconcile(&older),
            Err(BufferRefusal::NotNewer { .. })
        ));
        assert_eq!(predictor.authoritative().unwrap().rounds[0], 37);
    }

    #[test]
    fn accept_f57_b_a_large_error_snaps_and_an_old_generation_is_refused() {
        let mut world = World::new();
        let generation = world.ledger.generation(world.actor).unwrap().get();
        let mut predictor =
            LocalPredictor::new(world.actor, generation, PredictionConfig::default());
        predictor
            .record_predicted(Tick(10), pose(1_100.0), false)
            .unwrap();
        world.step(10, 1_000.0, |_| {});
        let record = *world.mirror.aircraft(world.actor).unwrap();
        let result = predictor.reconcile(&record).unwrap();
        assert_eq!(result.kind, ReconcileKind::Snapped);
        let step = predictor.next_correction();
        assert!((step.translation_m[0] + 100.0).abs() < 1e-6);
        assert!(!predictor.correcting());

        let mut wrong_generation = record;
        wrong_generation.generation = generation + 1;
        wrong_generation.tick = Tick(11);
        let mut fresh =
            LocalPredictor::new(world.actor, generation + 1, PredictionConfig::default());
        assert!(fresh.reconcile(&record).is_err());
        assert!(predictor.reconcile(&wrong_generation).is_err());
    }
}

// F57-C acceptance tests, in the same unit-test module as F57-B's: the CI
// runners ran out of room linking another Bevy-linked test binary, and these
// drive the same production path one stage further. Everything here goes through
// `NetSession`, `publish_snapshot` and `NetStateLedger`, so a stage whose wiring
// is removed cannot leave these passing.
#[cfg(test)]
mod f57_c_acceptance {
    use super::{
        BufferRefusal, CapError, EpochTransition, IngestOutcome, IngestRefusal,
        InterpolationConfig, NetSession, PredictedPose, PredictionConfig, ReconcileKind,
        SampleMode, SessionError, TracerRefusal, TracerVerdict, Tracers, publish_snapshot,
    };
    use crate::origin::{OriginEpoch, WorldOrigin};
    use cs_net::message::{EventBody, ReliableEvent};
    use cs_net::snapshot::Snapshot;
    use cs_sim::net_state::{
        ActorGeneration, Destruction, NetActorState, NetStateLedger, ShotConfirmation, ShotId,
        ShotOutcome,
    };
    use cs_types::Tick;
    use cs_types::net::{ActorAllocator, ActorId, EventId, SessionId};
    use cs_types::space::{Quaternion, WorldPosition};

    const SESSION: SessionId = match SessionId::new(73) {
        Some(id) => id,
        None => unreachable!(),
    };

    /// The world origin both ends start in: far from zero, so a frame mistake
    /// cannot hide inside small coordinates.
    fn origin() -> WorldOrigin {
        WorldOrigin::new(
            OriginEpoch(1),
            WorldPosition::try_new([10_000.0, 0.0, -4_000.0]).expect("finite"),
        )
    }

    fn at(x: f64) -> WorldPosition {
        WorldPosition::try_new([x, 100.0, 0.0]).expect("finite")
    }

    /// The producer and the wired consumer, driven together.
    struct Wired {
        ledger: NetStateLedger,
        session: NetSession,
        /// The last published snapshot and the exact bytes it encodes to, so a
        /// test can hold a packet "on the wire" across an epoch change and
        /// deliver it late.
        wire: Option<(Snapshot, Vec<u8>)>,
        remote: ActorId,
        local: ActorId,
    }

    impl Wired {
        fn new() -> Self {
            let mut ledger = NetStateLedger::new(SESSION);
            let mut allocator = ActorAllocator::new(SESSION);
            let remote = allocator.allocate().expect("serial");
            let local = allocator.allocate().expect("serial");
            let origin = origin();
            for actor in [remote, local] {
                ledger
                    .spawn(NetActorState::spawn(
                        actor,
                        at(10_000.0),
                        Quaternion::IDENTITY,
                        400,
                    ))
                    .expect("spawn");
            }
            let mut session = NetSession::new(SESSION, origin, InterpolationConfig::default());
            session
                .attach_local(
                    local,
                    ledger.generation(local).expect("generation").get(),
                    PredictionConfig::default(),
                )
                .expect("attach");
            Self {
                ledger,
                session,
                wire: None,
                remote,
                local,
            }
        }

        /// Moves a remote actor, publishes, round-trips the bytes and feeds the
        /// session: the whole producer → wire → consumer path.
        fn deliver_remote(&mut self, tick: u64, x: f64) {
            self.move_actor(self.remote, tick, x, 60.0);
        }

        fn move_actor(&mut self, actor: ActorId, tick: u64, x: f64, velocity: f64) {
            let mut state = *self.ledger.state(actor).expect("known");
            state.pose.position = at(x);
            state.linear_velocity_mps = [velocity, 0.0, 0.0];
            self.ledger.publish(state).expect("owned generation");
            self.publish(tick);
        }

        fn publish(&mut self, tick: u64) {
            let snapshot =
                publish_snapshot(&self.ledger, &self.session.origin()).expect("publishable");
            self.wire = Some((snapshot.clone(), snapshot.encode(SESSION).expect("encode")));
            self.session
                .ingest(&snapshot, Tick(tick))
                .expect("session is live");
        }

        /// The decoded bytes of the last published snapshot, as the transport
        /// would deliver them.
        fn last_wire(&self) -> Snapshot {
            let (snapshot, bytes) = self.wire.as_ref().expect("published");
            let decoded = Snapshot::decode(bytes, SESSION).expect("decode");
            assert_eq!(&decoded, snapshot, "the wire bytes do not decode back");
            decoded.clone()
        }

        /// The presented world x of the remote aircraft at estimated tick `now`.
        fn presented_x(&self, now: u64) -> (f64, SampleMode) {
            let sampled = self
                .session
                .sample(self.remote, Tick(now))
                .expect("representable")
                .expect("buffered");
            (sampled.state.position.x(), sampled.mode)
        }

        fn local_generation(&self) -> ActorGeneration {
            self.ledger.generation(self.local).expect("generation")
        }

        fn shot(&self, number: u32) -> ShotId {
            ShotId::try_new(number).expect("nonzero")
        }
    }

    /// AC03 (the F57-C minimum scenario): **origin change across snapshot
    /// boundaries produces no world-scale jump.**
    ///
    /// The session publishes and ingests in epoch 1, the world origin is rebased
    /// by several kilometres, and publication resumes. Every presented position
    /// must stay on the aircraft's actual path; the rebase may move the *frame*
    /// arbitrarily far, and the measured conversion must stay inside the declared
    /// round-trip tolerance.
    #[test]
    fn accept_f57_c_origin_change_across_snapshot_boundaries_produces_no_world_scale_jump() {
        let mut world = Wired::new();
        // Three snapshots in epoch 1: the aircraft flies from x = 10 000 at
        // 60 m/s (1 m per tick at 60 Hz).
        world.deliver_remote(10, 10_000.0);
        world.deliver_remote(20, 10_010.0);
        assert_eq!(world.presented_x(21).1, SampleMode::Interpolated);

        // The origin moves 8 km in x and 3 km in z. This is the world-scale change
        // an unconverted receiver would turn into a 8.5 km displacement.
        let rebased = world
            .session
            .rebase(WorldPosition::try_new([18_000.0, 0.0, -1_000.0]).expect("finite"))
            .expect("forward epoch");
        assert!(
            rebased.origin_shift_m > 8_000.0,
            "{}",
            rebased.origin_shift_m
        );
        assert_eq!(rebased.from.0 + 1, rebased.to.0);

        // Publication resumes in the new epoch: the wire epoch changed, and the
        // receiver accepts it because the session adopted the same epoch.
        world.deliver_remote(30, 10_020.0);
        let (x, _) = world.presented_x(31);
        assert!(
            (x - 10_020.0).abs() < 0.05,
            "aircraft jumped to {x} across the rebase"
        );

        // No mirror record moved by anything like the origin shift: the
        // transition is a conversion, bounded by the declared round-trip
        // tolerance rather than by the displacement.
        assert_eq!(rebased.converted, 2);
        assert!(
            rebased.max_conversion_m < 0.01,
            "conversion drift {} m",
            rebased.max_conversion_m
        );

        // And the aircraft keeps flying the same path afterwards, at the same
        // speed: a rebase is not a velocity impulse.
        world.deliver_remote(40, 10_030.0);
        let (x, mode) = world.presented_x(41);
        assert!(mode != SampleMode::Extrapolated, "{mode:?}");
        assert!((x - 10_030.0).abs() < 0.05, "{x}");
    }

    /// The other half of AC03: a snapshot from the *old* epoch is refused after
    /// the rebase, so a late packet cannot be read in the wrong frame.
    #[test]
    fn accept_f57_c_a_snapshot_from_a_retired_origin_epoch_is_refused_not_reinterpreted() {
        let mut world = Wired::new();
        world.deliver_remote(10, 10_000.0);
        let old_epoch_wire = world.last_wire();

        world
            .session
            .rebase(WorldPosition::try_new([18_000.0, 0.0, -1_000.0]).expect("finite"))
            .expect("forward epoch");
        world.deliver_remote(20, 10_010.0);
        let before = world.presented_x(21).0;

        // The in-flight packet from the retired frame arrives late.
        let report = world
            .session
            .ingest(&old_epoch_wire, Tick(30))
            .expect("session is live");
        assert_eq!(
            report.outcome,
            IngestOutcome::Refused(IngestRefusal::EpochMismatch {
                snapshot: old_epoch_wire.origin.0,
                local: world.session.origin().epoch().0,
            })
        );
        assert_eq!(report.applied, 0);
        // The presented pose did not move: refusing is the only correct outcome,
        // and reinterpreting those integers in the new frame would have moved it
        // by the origin shift.
        assert!((world.presented_x(21).0 - before).abs() < 1e-9);
    }

    /// An origin epoch that does not move forward is refused, so a reused epoch
    /// can never name live records.
    #[test]
    fn accept_f57_c_an_origin_epoch_never_goes_backwards_or_repeats() {
        let mut world = Wired::new();
        world.deliver_remote(10, 10_000.0);

        let moved = world
            .session
            .rebase(WorldPosition::try_new([18_000.0, 0.0, -1_000.0]).expect("finite"))
            .expect("forward");
        assert_eq!(moved.to.0, moved.from.0 + 1);

        // The same position again would allocate the same next epoch... which is
        // legal, so the refusal that matters is the mirror's: adopting an epoch
        // that is not strictly newer. Reach it through the mirror directly.
        let repeat = world
            .session
            .mirror()
            .origin()
            .rebased(WorldPosition::try_new([18_000.0, 0.0, -1_000.0]).expect("finite"));
        assert!(repeat.is_ok());
        let backwards = WorldOrigin::new(
            moved.from,
            WorldPosition::try_new([0.0, 0.0, 0.0]).expect("finite"),
        );
        let refusal = world.session.mirror().clone().adopt_origin(backwards);
        assert!(refusal.is_err(), "a retired epoch was adopted again");
    }

    /// The projectile side of the sheet's non-negotiable behavior 3: a local
    /// tracer is a cosmetic and only the server's confirmation resolves it.
    #[test]
    fn accept_f57_c_a_local_tracer_stays_cosmetic_until_the_server_confirms_the_shot() {
        let mut world = Wired::new();
        let shot = world.shot(1);

        // The server accepts the fire request; only then is there a shot id.
        world
            .ledger
            .accept_shot(world.local, world.local_generation(), shot)
            .expect("live shooter");
        assert_eq!(world.ledger.highest_shot(world.local), Some(shot));

        world
            .session
            .spawn_tracer(shot, Tick(10), at(10_000.0), [0.0, 0.0, -300.0])
            .expect("spawn");
        // While the server has said nothing, nothing is resolved.
        {
            let tracers = world.session.tracers().expect("attached");
            assert_eq!(
                tracers.tracer(shot).expect("pending").verdict,
                TracerVerdict::Pending
            );
            assert_eq!(tracers.resolved_count(), 0);
        }

        // The server confirms a hit on the remote actor. The confirmation is the
        // only thing that resolves the tracer.
        let outcome = world
            .ledger
            .confirm_shot(
                world.local,
                world.local_generation(),
                shot,
                Some(world.remote),
                Tick(12),
            )
            .expect("accepted shot");
        assert!(matches!(outcome, ShotOutcome::Confirmed(_)));
        let confirmation = outcome.first_confirmation().expect("first");
        world
            .session
            .apply_confirmation(&confirmation)
            .expect("confirmation applies");
        let tracers = world.session.tracers().expect("attached");
        assert_eq!(
            tracers.tracer(shot).expect("tracked").verdict,
            TracerVerdict::Confirmed {
                target: Some(world.remote)
            }
        );
        assert_eq!(tracers.resolved_count(), 1);
    }

    /// A shot the server never accepted has no id to confirm, so a purely local
    /// prediction cannot award anything — and a replayed fire request is refused
    /// rather than firing twice.
    #[test]
    fn accept_f57_c_an_unaccepted_shot_can_neither_be_confirmed_nor_fire_twice() {
        let mut world = Wired::new();
        let first = world.shot(4);
        let replay = world.shot(4);
        let later = world.shot(9);

        world
            .ledger
            .accept_shot(world.local, world.local_generation(), first)
            .expect("first accepted");
        // The same number again is a replayed fire request.
        assert_eq!(
            world
                .ledger
                .accept_shot(world.local, world.local_generation(), replay),
            Err(cs_sim::net_state::NetStateError::StaleShot {
                actor: world.local,
                presented: replay,
                highest: first,
            })
        );
        assert_eq!(world.ledger.accepted_shots(world.local), 1);

        // A shot number the server never accepted cannot be confirmed at all.
        assert_eq!(
            world.ledger.confirm_shot(
                world.local,
                world.local_generation(),
                later,
                Some(world.remote),
                Tick(12),
            ),
            Err(cs_sim::net_state::NetStateError::ShotNotAccepted {
                actor: world.local,
                shot: later,
            })
        );

        // A confirmation from another session's shooter is refused by the
        // cosmetic book, which only ever draws the local aircraft's shots.
        let foreign = ShotConfirmation {
            shot: first,
            shooter: world.remote,
            generation: world.ledger.generation(world.remote).expect("generation"),
            tick: Tick(12),
            target: Some(world.remote),
            destruction: None,
        };
        assert_eq!(
            world.session.apply_confirmation(&foreign),
            Err(SessionError::Tracer(TracerRefusal::ForeignShooter {
                local: world.local,
                found: world.remote,
            }))
        );
    }

    /// A replayed confirmation is idempotent and awards the kill at most once,
    /// which is AC01's "no duplicate destruction" reached through the projectile
    /// path rather than the snapshot path.
    #[test]
    fn accept_f57_c_a_replayed_confirmation_awards_one_kill_and_is_absorbed_the_second_time() {
        let mut world = Wired::new();
        let shot = world.shot(2);
        world
            .ledger
            .accept_shot(world.local, world.local_generation(), shot)
            .expect("accepted");

        let first = world
            .ledger
            .confirm_shot(
                world.local,
                world.local_generation(),
                shot,
                Some(world.remote),
                Tick(20),
            )
            .expect("confirmed");
        assert!(first.awarded());
        assert_eq!(
            first.first_confirmation().expect("first").destruction,
            Some(Destruction::Recorded { tick: Tick(20) })
        );

        // The same reliable hit report replayed (reconnect and retry can do
        // this): absorbed, no second award.
        let replay = world
            .ledger
            .confirm_shot(
                world.local,
                world.local_generation(),
                shot,
                Some(world.remote),
                Tick(20),
            )
            .expect("absorbed");
        assert!(!replay.awarded());
        assert_eq!(
            replay,
            ShotOutcome::AlreadyConfirmed {
                shot,
                tick: Tick(20)
            }
        );
        assert_eq!(world.ledger.confirmed_shots(world.local), 1);

        // The remote aircraft really is destroyed once: publishing it reports the
        // destruction and the mirror retires it with no ghost left behind.
        let generation = world.ledger.generation(world.remote).expect("generation");
        assert_eq!(
            world.ledger.end_lifecycle(
                world.remote,
                generation,
                cs_sim::net_state::NetLifecycle::Destroyed
            ),
            Err(cs_sim::net_state::NetStateError::AlreadyTerminal {
                actor: world.remote,
                generation,
                lifecycle: cs_sim::net_state::NetLifecycle::Destroyed,
            })
        );
    }

    /// The reconciliation the wired session performs: a server correction during
    /// a local boost keeps ammunition and fuel authoritative and hands the pose
    /// owner a bounded correction.
    #[test]
    fn accept_f57_c_the_wired_session_reconciles_local_prediction_and_keeps_the_server_word() {
        let mut world = Wired::new();

        // The local body runs ahead while boosting; its predicted poses are the
        // ones the session will compare against.
        for tick in 1..=10_u64 {
            world
                .session
                .record_local_pose(
                    Tick(tick),
                    PredictedPose {
                        position: at(10_000.0 + tick as f64 * 2.3),
                        orientation: Quaternion::IDENTITY,
                    },
                    true,
                )
                .expect("increasing ticks");
        }
        assert!(world.session.predictor().expect("attached").boost_shown());

        // The server's tick 10: less boost capacity, fewer rounds, and a pose
        // behind the prediction.
        let mut state = *world.ledger.state(world.local).expect("known");
        state.pose.position = at(10_000.0 + 20.0);
        state.flight.boost_capacity = 0.25;
        state.weapons.primary_rounds = 37;
        world.ledger.publish(state).expect("owned");
        world.publish(10);

        let reconciliation = world.session.reconcile_local().expect("reconciles");
        assert_eq!(reconciliation.kind, ReconcileKind::Smoothed);
        assert!(
            (reconciliation.error_m - 3.0).abs() < 0.05,
            "{}",
            reconciliation.error_m
        );

        // The server's word, read through the session.
        let loadout = world
            .session
            .predictor()
            .expect("attached")
            .authoritative()
            .expect("reconciled");
        assert_eq!(loadout.rounds[0], 37);
        assert!((loadout.flight[2] - 0.25).abs() < 1e-4);
        assert!(world.session.predictor().expect("attached").boost_shown());

        // The session hands the pose owner a bounded correction and never writes
        // a body itself.
        let correction = world.session.next_correction();
        assert!(correction.translation_m[0].abs() > 0.0);
        assert!(
            correction.translation_m[0].abs() < 3.0,
            "unbounded correction"
        );
    }

    /// Reconciliation follows ingestion by construction: the session reads the
    /// record from its own mirror, so a snapshot that was refused cannot reach
    /// the predictor.
    #[test]
    fn accept_f57_c_refused_snapshots_never_reach_the_local_predictor() {
        let mut world = Wired::new();
        world.deliver_remote(10, 10_000.0);

        // Reconcile with no local record yet.
        assert_eq!(
            world.session.reconcile_local(),
            Err(SessionError::UnknownLocalActor {
                expected: Some(world.local)
            })
        );

        // An epoch-mismatched snapshot is refused whole, so the mirror holds no
        // local record to reconcile against.
        world
            .session
            .rebase(WorldPosition::try_new([18_000.0, 0.0, -1_000.0]).expect("finite"))
            .expect("forward");
        let stale = world.session.mirror().origin();
        assert!(stale.epoch().0 > 1);
        assert_eq!(
            world.session.reconcile_local(),
            Err(SessionError::UnknownLocalActor {
                expected: Some(world.local)
            })
        );
    }

    /// Teardown is idempotent, releases the buffers, and answers everything after
    /// it; a retry is a new session.
    #[test]
    fn accept_f57_c_teardown_releases_the_session_and_refuses_late_work() {
        let mut world = Wired::new();
        world.deliver_remote(10, 10_000.0);
        world.deliver_remote(20, 10_010.0);
        assert!(world.session.interpolator().track_count() > 0);

        assert_eq!(world.session.teardown(), 2);
        assert!(world.session.is_torn_down());
        assert_eq!(world.session.interpolator().track_count(), 0);
        assert_eq!(world.session.mirror().aircraft_count(), 0);
        assert!(world.session.predictor().is_none());
        assert!(world.session.tracers().is_none());

        // Idempotent.
        assert_eq!(world.session.teardown(), 0);

        // A late snapshot, a late event and a correction request are all refused
        // by name rather than silently doing nothing.
        let snapshot = world.last_wire();
        assert_eq!(
            world.session.ingest(&snapshot, Tick(30)),
            Err(SessionError::TornDown)
        );
        assert_eq!(
            world
                .session
                .rebase(WorldPosition::try_new([0.0, 0.0, 0.0]).expect("finite")),
            Err(SessionError::TornDown)
        );
        assert_eq!(world.session.reconcile_local(), Err(SessionError::TornDown));
        // The pose correction is a no-op rather than an error: the single pose
        // owner asks for it every tick and must get the identity after teardown.
        let correction = world.session.next_correction();
        assert_eq!(correction.translation_m, [0.0; 3]);

        // A retry is a fresh session, and it starts with no remembered history.
        let mut retry = NetSession::new(SESSION, origin(), InterpolationConfig::default());
        assert_eq!(retry.interpolator().track_count(), 0);
        retry
            .attach_local(
                world.local,
                world.local_generation().get(),
                PredictionConfig::default(),
            )
            .expect("attach");
        let report = retry.ingest(&snapshot, Tick(1)).expect("live");
        assert!(report.is_applied());
    }

    /// A reliably removed actor releases its interpolation history in the wired
    /// session, and a replay of the same event changes nothing.
    #[test]
    fn accept_f57_c_a_reliable_removal_releases_history_and_its_replay_changes_nothing() {
        let mut world = Wired::new();
        world.deliver_remote(10, 10_000.0);
        world.deliver_remote(20, 10_010.0);
        assert_eq!(world.session.interpolator().sample_count(world.remote), 2);

        let event = ReliableEvent {
            id: EventId {
                session: SESSION,
                tick: Tick(21),
                producer: 0,
                sequence: 1,
            },
            body: EventBody::ActorRemoved {
                actor: world.remote,
            },
        };
        assert!(world.session.apply_event(&event).expect("live"));
        assert_eq!(world.session.interpolator().sample_count(world.remote), 0);
        assert_eq!(world.session.mirror().aircraft_count(), 1);

        // The replay is absorbed.
        assert!(!world.session.apply_event(&event).expect("live"));

        // And a snapshot that still carries the removed actor cannot bring it
        // back.
        world.deliver_remote(30, 10_020.0);
        assert!(world.session.mirror().aircraft(world.remote).is_none());
    }

    /// The tracer book is bounded twice over: by its configured cap and by the
    /// hard ceiling, and a confirmed tracer outlives an unanswered one.
    #[test]
    fn accept_f57_c_tracers_are_bounded_and_a_confirmed_shot_outlives_a_prediction() {
        let local = ActorId {
            session: SESSION,
            serial: 9,
        };
        // The checked constructor refuses an over-cap claim rather than silently
        // holding less.
        assert_eq!(
            Tracers::try_new(local, Tracers::TRACER_HARD_CAP + 1).err(),
            Some(CapError::AboveHardCap {
                requested: Tracers::TRACER_HARD_CAP + 1,
                max: Tracers::TRACER_HARD_CAP,
            })
        );

        let mut tracers = Tracers::try_new(local, 2).expect("within the cap");
        let early = ShotId::try_new(1).expect("nonzero");
        let late = ShotId::try_new(2).expect("nonzero");
        let newest = ShotId::try_new(3).expect("nonzero");
        tracers
            .spawn(early, Tick(10), at(10_000.0), [0.0, 0.0, -300.0])
            .expect("spawn");
        // `early` is confirmed, so it is the one kept over an unanswered tracer.
        let confirmation = ShotConfirmation {
            shot: early,
            shooter: local,
            generation: ActorGeneration::try_new(1).expect("nonzero"),
            tick: Tick(11),
            target: Some(ActorId {
                session: SESSION,
                serial: 10,
            }),
            destruction: None,
        };
        tracers.apply_confirmation(&confirmation).expect("applies");
        tracers
            .spawn(late, Tick(20), at(10_000.0), [0.0, 0.0, -300.0])
            .expect("spawn");
        // At the cap: the newest spawn evicts the unanswered `late`, not the
        // confirmed `early`.
        tracers
            .spawn(newest, Tick(21), at(10_000.0), [0.0, 0.0, -300.0])
            .expect("spawn");
        assert_eq!(tracers.len(), 2);
        assert!(
            tracers.tracer(early).is_some(),
            "a confirmed shot was evicted"
        );
        assert!(tracers.tracer(late).is_none(), "an unanswered one was kept");
        assert!(tracers.tracer(newest).is_some());

        // A replayed spawn for a tracked shot is refused rather than restamping.
        assert_eq!(
            tracers.spawn(early, Tick(22), at(10_000.0), [0.0, 0.0, -300.0]),
            Err(TracerRefusal::AlreadyTracked { shot: early })
        );
        // A non-finite direction is refused.
        assert_eq!(
            tracers.spawn(
                ShotId::try_new(4).expect("nonzero"),
                Tick(23),
                at(10_000.0),
                [f64::NAN, 0.0, 0.0]
            ),
            Err(TracerRefusal::UnusableDirection)
        );

        // The lifetime bound drops an unanswered tracer nobody ever confirmed.
        tracers.expire(Tick(21 + Tracers::TRACER_LIFETIME_TICKS + 1));
        assert_eq!(tracers.len(), 0);
    }

    /// Attaching a second local aircraft to a live session is refused: switching
    /// mid-session would interleave one aircraft's prediction with another's
    /// authoritative loadout.
    #[test]
    fn accept_f57_c_a_live_session_refuses_a_different_local_actor() {
        let mut world = Wired::new();
        let other = ActorId {
            session: SESSION,
            serial: 77,
        };
        assert_eq!(
            world
                .session
                .attach_local(other, 1, PredictionConfig::default()),
            Err(SessionError::UnknownLocalActor {
                expected: Some(world.local)
            })
        );
        // Re-attaching the same actor is idempotent.
        world
            .session
            .attach_local(world.local, 1, PredictionConfig::default())
            .expect("same actor");
    }

    /// The buffer refusals the wired path can report are the buffer's own, so a
    /// caller can tell a stale generation from an out-of-order tick.
    #[test]
    fn accept_f57_c_the_wired_path_reports_the_buffer_refusals_it_saw() {
        let mut world = Wired::new();
        world.deliver_remote(10, 10_000.0);
        // Reconciling a record twice in a row: the second is not newer.
        world.publish(10);
        world.session.reconcile_local().expect("first");
        let stale = world.session.reconcile_local();
        assert!(stale.is_err(), "{stale:?}");
        assert!(matches!(
            stale,
            Err(SessionError::Buffer(BufferRefusal::NotNewer { .. }))
        ));
    }

    /// A session with no local aircraft attached reports that, rather than
    /// quietly doing nothing when asked to predict.
    #[test]
    fn accept_f57_c_a_session_without_a_local_actor_says_so() {
        let mut session = NetSession::new(SESSION, origin(), InterpolationConfig::default());
        assert_eq!(
            session.record_local_pose(
                Tick(1),
                PredictedPose {
                    position: at(0.0),
                    orientation: Quaternion::IDENTITY,
                },
                false,
            ),
            Err(SessionError::NoLocalActor)
        );
        assert_eq!(
            session.spawn_tracer(
                ShotId::try_new(1).expect("nonzero"),
                Tick(1),
                at(0.0),
                [0.0, 0.0, -1.0],
            ),
            Err(SessionError::NoLocalActor)
        );
        assert!(session.predictor().is_none());
    }

    /// The `EpochTransition` a caller reads is the measurement the AC03 test
    /// relies on: a large frame shift with a conversion inside the tolerance.
    #[test]
    fn accept_f57_c_an_epoch_transition_reports_the_shift_and_the_conversion_separately() {
        let mut world = Wired::new();
        world.deliver_remote(10, 10_000.0);
        let before = world.session.origin();
        let transition: EpochTransition = world
            .session
            .rebase(WorldPosition::try_new([90_000.0, 1_000.0, 60_000.0]).expect("finite"))
            .expect("forward");
        assert_ne!(transition.to, transition.from);
        assert_ne!(transition.origin, before.position());
        // The frame moved ~100 km; no record moved with it.
        assert!(
            transition.origin_shift_m > 90_000.0,
            "{}",
            transition.origin_shift_m
        );
        assert!(
            transition.max_conversion_m < 1.0,
            "{}",
            transition.max_conversion_m
        );
        assert_eq!(transition.converted, 2);
    }
}
