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
