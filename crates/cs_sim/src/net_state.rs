//! Authoritative networked state, generations and input acknowledgment (F57-A).
//!
//! Spec: `specs/F57-networked-aircraft-prediction-interpolation-and-projectiles.md`,
//! stage `### F57-A`. Contracts: `docs/contracts/UI-NETWORK.md`
//! ("Server owns ActorId allocation, physics truth, weapon acceptance,
//! hit/damage, faction/interaction, mission program, score and result",
//! "Inputs have sequence acknowledgment and bounded acceptance windows",
//! "Interpolation buffers separate actor generations") and
//! `docs/contracts/FLIGHT-PHYSICS.md` ("Reject nonfinite inputs at boundaries",
//! "Commands belong to one simulation tick and actor generation").
//!
//! # What this is
//!
//! The *truth* a snapshot publishes and the *rules* about who may change it.
//! [`NetStateLedger`] is the per-session authority: it allocates each actor's
//! [`ActorGeneration`], owns every actor's authoritative
//! [`NetActorState`], records a destruction exactly once per generation, and
//! carries the input acknowledgment a client needs to stop resending. Nothing
//! here is client-authored state, and there is no field a client can write: a
//! client sends bounded tick-stamped input (F54-A) and receives snapshots.
//!
//! # Generations, and why ids alone are not enough
//!
//! [`cs_types::net::ActorId`] serials are never recycled inside a session, so an
//! id alone names one actor for the session's lifetime. A generation is still
//! carried on the wire and in every state, because "never share history across
//! generations" has to be checkable *at the boundary* rather than assumed from
//! an allocator's discipline: a host migration, a reconnect to a restored
//! session or a future policy change must not be able to hand a receiving
//! buffer an id whose earlier history it still holds. [`ActorGeneration`] is
//! therefore a checked value ([`ActorGeneration::try_new`] refuses zero) and
//! [`NetStateLedger::publish`] refuses a caller that presents a generation the
//! ledger does not currently own ([`NetStateError::StaleGeneration`]).
//!
//! # Destruction is awarded once
//!
//! The damage domain already awards one kill per destruction
//! ([`crate::damage::DamageResolver`]); this ledger is the second, independent
//! gate the network path needs. [`NetStateLedger::record_destruction`] returns
//! [`Destruction::Recorded`] the first time and [`Destruction::AlreadyRecorded`]
//! for every later report of the same `(actor, generation)`, whatever the tick
//! and whatever reported it. A snapshot may therefore carry the `Destroyed`
//! flag as often as retransmission likes — the ledger's dedup state, not the
//! packet count, is what makes a kill single (F57 AC01: "no duplicate
//! destruction"). Dedupe state is bounded by actor lifetime: it lives with the
//! actor and is dropped by [`NetStateLedger::forget`], never accumulated in an
//! unbounded set (`docs/01-ARCHITECTURE.md`, "Sound, score, capture,
//! destruction and reward consumers keep appropriate deduplication state").
//!
//! # Ammunition and fuel stay server-side
//!
//! [`NetWeapons`] and [`NetFlight`] are ordinary authoritative fields: the only
//! way they change is a method on [`NetActorState`], and no method accepts a
//! client packet. The input path ([`NetStateLedger::observe_input`]) advances
//! only the acknowledgment, so a client-side prediction of a shot or a boost
//! can never consume a round or boost capacity here (F57 AC02, AC03's
//! authority half). Reconciliation of *motion* is F57-B; this stage fixes what
//! stays authoritative.
//!
//! # Shots are proposed by the client and accepted here (F57-C)
//!
//! [`NetStateLedger::accept_shot`] is the weapon-acceptance gate: the client
//! proposes a [`ShotId`] with its fire intent, the server accepts it only if it
//! is strictly newer than every shot it already accepted, and
//! [`NetStateLedger::confirm_shot`] will only confirm a shot that passed that
//! gate. A client-drawn tracer whose shot was refused therefore has nothing that
//! can confirm it, and a replayed confirmation is absorbed by the constant-size
//! shot book — so a confirmed hit and an awarded kill cannot come apart, and
//! two shots hitting one actor award one kill through
//! [`NetStateLedger::record_destruction`].
//!
//! All values are newly authored engine design in SI units and radians
//! (`docs/contracts/FLIGHT-PHYSICS.md`): no original networked flight, weapon or
//! destruction behavior has been measured, and nothing here asserts any.

use std::collections::BTreeMap;
use std::fmt;

use cs_types::Tick;
use cs_types::net::{ActorId, EventId, SessionId};
use cs_types::space::{Quaternion, WorldPosition};

/// A nonzero actor generation.
///
/// Zero is refused for the same reason [`cs_types::net::SessionId`] refuses it:
/// a default or truncated field must never alias a live actor generation, and
/// "generation 0" would be indistinguishable from "generation not carried".
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct ActorGeneration(u16);

impl ActorGeneration {
    /// Wraps an allocated generation number; zero is refused.
    pub const fn try_new(value: u16) -> Option<Self> {
        if value == 0 { None } else { Some(Self(value)) }
    }

    /// The generation number.
    pub const fn get(self) -> u16 {
        self.0
    }

    /// The next generation after this one.
    ///
    /// # Errors
    ///
    /// [`NetStateError::GenerationExhausted`] when the counter would wrap; a
    /// generation must never name two different actors' histories.
    pub const fn next(self) -> Result<Self, NetStateError> {
        match self.0.checked_add(1) {
            Some(value) => match Self::try_new(value) {
                Some(generation) => Ok(generation),
                None => Err(NetStateError::GenerationExhausted),
            },
            None => Err(NetStateError::GenerationExhausted),
        }
    }
}

impl fmt::Display for ActorGeneration {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "generation {}", self.0)
    }
}

/// What the server last did with an actor.
///
/// The damage domain distinguishes five lifecycle kinds
/// ([`crate::damage::LifecycleKind`]); the network state needs only the
/// question a receiver asks — "is this actor still here?" — plus the reason it
/// is not, because a bailout and a destruction are different events to
/// presentation, scoring and objective consumers even though both end the
/// actor's record.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum NetLifecycle {
    /// The actor exists and is being simulated.
    Alive,
    /// The actor was destroyed by damage.
    Destroyed,
    /// The pilot left the airframe without destroying it.
    BailedOut,
    /// The actor's entities left the world (mission removal, capture, unload).
    Despawned,
}

impl NetLifecycle {
    /// Every kind, in a stable order.
    pub const ALL: &'static [NetLifecycle] = &[
        Self::Alive,
        Self::Destroyed,
        Self::BailedOut,
        Self::Despawned,
    ];

    /// The stable label used in reports.
    pub const fn label(self) -> &'static str {
        match self {
            Self::Alive => "alive",
            Self::Destroyed => "destroyed",
            Self::BailedOut => "bailed_out",
            Self::Despawned => "despawned",
        }
    }

    /// Whether the kind ends the actor's record: after it, no later report for
    /// the same generation may bring the actor back.
    pub const fn is_terminal(self) -> bool {
        !matches!(self, Self::Alive)
    }
}

impl fmt::Display for NetLifecycle {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.label())
    }
}

/// Who is steering an actor.
///
/// A designed minimum so a snapshot can tell a pilot from a scripted aircraft;
/// the original's own control modes are unmeasured, so nothing finer is
/// invented here.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum NetControlMode {
    /// A pilot, or a player-controlled aircraft.
    Manual,
    /// The mission program or an AI is steering.
    Autopilot,
    /// Nobody is steering; the airframe is coasting or falling.
    Uncontrolled,
}

impl NetControlMode {
    /// Every mode, in a stable order.
    pub const ALL: &'static [NetControlMode] = &[Self::Manual, Self::Autopilot, Self::Uncontrolled];

    /// The stable label used in reports.
    pub const fn label(self) -> &'static str {
        match self {
            Self::Manual => "manual",
            Self::Autopilot => "autopilot",
            Self::Uncontrolled => "uncontrolled",
        }
    }
}

/// An actor's authoritative pose in canonical world space.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct NetPose {
    /// World position in meters, relative to whichever world-origin epoch the
    /// surrounding context declares. Quantization relative to a *shared* epoch
    /// is the network side of F16's world-origin rule; this value stays f64
    /// here and is quantized by the publishing boundary.
    pub position: WorldPosition,
    /// Body-to-world orientation.
    pub orientation: Quaternion,
}

/// An actor's authoritative flight state.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct NetFlight {
    /// Commanded throttle in `[0, 1]`.
    pub throttle: f64,
    /// Engine spool in `[0, 1]`.
    pub engine_spool: f64,
    /// Boost capacity still available in `[0, 1]`. This is a *capacity*, so a
    /// refused boost press cannot have spent it
    /// (`docs/contracts/FLIGHT-PHYSICS.md`, "Boost and special models").
    pub boost_capacity: f64,
}

/// An actor's authoritative damage summary.
///
/// A summary, not the damage graph: how much structure is left and which weapon
/// mounts are disabled. Damage *award* is the resolver's
/// (`crate::damage::DamageResolver`), server-side, and no networked input
/// reaches this struct.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct NetDamage {
    /// Remaining structural integrity as a fraction of full, in `[0, 1]`.
    pub remaining: f64,
    /// Bit mask of the weapon mounts the damage resolver has disabled.
    pub disabled_mounts: u16,
}

/// An actor's authoritative weapon state.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct NetWeapons {
    /// Rounds in the primary bank.
    pub primary_rounds: u32,
    /// Rounds in the secondary bank.
    pub secondary_rounds: u32,
    /// Which bank the trigger is on: `0` primary, `1` secondary.
    pub selected_bank: u8,
}

impl NetWeapons {
    /// The largest round count a bank may hold.
    ///
    /// Bounded because the value is published in a fixed-width snapshot field
    /// (`cs_net::snapshot::ROUNDS_QUANTIZATION`, 16 unsigned bits): a count
    /// beyond what the wire can carry has to be refused here, where the
    /// authoritative number still exists, rather than truncated into a
    /// plausible different count on the way out.
    pub const MAX_ROUNDS: u32 = u16::MAX as u32;

    /// The banks a trigger may select.
    pub const BANK_CODES: [u8; 2] = [0, 1];
}

/// One actor's complete authoritative network state.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct NetActorState {
    /// Which actor this state belongs to.
    pub actor: ActorId,
    /// Which generation of that actor. Never zero, and never a generation the
    /// ledger does not currently own.
    pub generation: ActorGeneration,
    /// What the server last did with the actor.
    pub lifecycle: NetLifecycle,
    /// Who is steering.
    pub control: NetControlMode,
    /// Authoritative pose.
    pub pose: NetPose,
    /// World linear velocity in m/s.
    pub linear_velocity_mps: [f64; 3],
    /// Body angular velocity in rad/s.
    pub angular_velocity_radps: [f64; 3],
    /// Authoritative flight state.
    pub flight: NetFlight,
    /// Authoritative damage summary.
    pub damage: NetDamage,
    /// Authoritative weapon state.
    pub weapons: NetWeapons,
}

/// Why an authoritative network-state operation was refused.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum NetStateError {
    /// An actor belongs to another session generation.
    ForeignSession {
        /// The session this ledger owns.
        expected: SessionId,
        /// The session the caller used.
        found: SessionId,
    },
    /// An actor id that is not a valid one (zero session or serial).
    InvalidActorId {
        /// The offending id.
        actor: ActorId,
    },
    /// The actor is not known to this ledger.
    UnknownActor {
        /// The actor.
        actor: ActorId,
    },
    /// The actor id is already known to this ledger. Actor serials are never
    /// recycled, so a second registration under one id is a caller bug.
    DuplicateActor {
        /// The actor.
        actor: ActorId,
    },
    /// The caller presented a generation the ledger does not currently own,
    /// which is what a stale packet or a reused buffer would look like.
    StaleGeneration {
        /// The actor.
        actor: ActorId,
        /// The generation the caller presented.
        presented: ActorGeneration,
        /// The generation the ledger owns.
        owned: ActorGeneration,
    },
    /// The actor's lifecycle already ended: no later report may bring it back
    /// or re-award its destruction.
    AlreadyTerminal {
        /// The actor.
        actor: ActorId,
        /// The generation.
        generation: ActorGeneration,
        /// The terminal lifecycle already recorded.
        lifecycle: NetLifecycle,
    },
    /// A reported input sequence was already acknowledged or precedes the
    /// current one ("Inputs have sequence acknowledgment", contract).
    StaleInput {
        /// The sequence the caller presented.
        presented: u32,
        /// The highest sequence already acknowledged.
        acknowledged: u32,
    },
    /// A value was non-finite or outside its declared range.
    OutOfRange {
        /// The offending field.
        field: &'static str,
    },
    /// A proposed shot number is not newer than one this ledger already
    /// accepted for the shooter: a replayed fire intent or a reordered one.
    /// Weapon acceptance is the server's, so the shot does not exist.
    StaleShot {
        /// The shooter whose fire intent was presented.
        actor: ActorId,
        /// The proposed shot number.
        presented: ShotId,
        /// The highest shot number already accepted for the shooter.
        highest: ShotId,
    },
    /// A confirmation names a shot the ledger never accepted for that shooter —
    /// including a shot the server itself refused. A hit report cannot invent a
    /// shot, so a purely predicted cosmetic can never be confirmed.
    ShotNotAccepted {
        /// The shooter the confirmation claims.
        actor: ActorId,
        /// The shot the confirmation names.
        shot: ShotId,
    },
    /// The generation counter would wrap.
    GenerationExhausted,
}

impl fmt::Display for NetStateError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::ForeignSession { expected, found } => write!(
                f,
                "actor belongs to {found}, but this ledger owns {expected}"
            ),
            Self::InvalidActorId { actor } => {
                write!(
                    f,
                    "{actor} is not a valid actor id: session and serial are nonzero"
                )
            }
            Self::UnknownActor { actor } => write!(f, "{actor} is not known to this ledger"),
            Self::DuplicateActor { actor } => {
                write!(
                    f,
                    "{actor} is already registered; actor serials are never recycled"
                )
            }
            Self::StaleGeneration {
                actor,
                presented,
                owned,
            } => write!(
                f,
                "{actor} was presented with {presented}, but this ledger owns {owned}"
            ),
            Self::AlreadyTerminal {
                actor,
                generation,
                lifecycle,
            } => write!(
                f,
                "{actor} {generation} already ended as {lifecycle}; no later report may apply"
            ),
            Self::StaleInput {
                presented,
                acknowledged,
            } => write!(
                f,
                "input sequence {presented} is not newer than the acknowledged {acknowledged}"
            ),
            Self::OutOfRange { field } => {
                write!(
                    f,
                    "authoritative field {field} is outside its declared range"
                )
            }
            Self::StaleShot {
                actor,
                presented,
                highest,
            } => write!(
                f,
                "{actor} proposed {presented}, which is not newer than the accepted {highest}"
            ),
            Self::ShotNotAccepted { actor, shot } => write!(
                f,
                "{actor} has no accepted {shot}, so a confirmation for it cannot be authoritative"
            ),
            Self::GenerationExhausted => {
                write!(f, "actor generation counter is exhausted for this ledger")
            }
        }
    }
}

impl std::error::Error for NetStateError {}

/// What one destruction report did.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Destruction {
    /// The first report of this `(actor, generation)`: the kill is awarded
    /// here.
    Recorded {
        /// The tick the destruction happened on.
        tick: Tick,
    },
    /// A later report of the same `(actor, generation)`. No second award, and
    /// no state change: a retransmitted snapshot or a second resolver report
    /// is absorbed here.
    AlreadyRecorded {
        /// The tick the destruction was first recorded on.
        tick: Tick,
    },
}

impl Destruction {
    /// Whether this report awarded the kill.
    pub const fn awarded(self) -> bool {
        matches!(self, Self::Recorded { .. })
    }

    /// The tick the destruction is recorded on, whether this report or an
    /// earlier one first recorded it.
    pub const fn tick(self) -> Tick {
        match self {
            Self::Recorded { tick } | Self::AlreadyRecorded { tick } => tick,
        }
    }
}

/// One accepted client input sequence, and the acknowledgment it produced.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct InputAck {
    /// The sequence the server consumed.
    pub sequence: u32,
    /// The tick the server consumed it on.
    pub tick: Tick,
}

// ---------------------------------------------------- shots and confirmation ----

/// A shooter's shot sequence number: the identity of one accepted fire intent.
///
/// Monotonically increasing per `(actor, generation)` and never zero. The
/// *client* proposes the number with its fire intent and the server decides
/// whether the shot exists ([`NetStateLedger::accept_shot`]), so the number is a
/// request identity rather than an authority: a proposed number the server
/// refuses names no shot, and a shot the server accepted is the only one a
/// confirmation can name. That is what keeps a predicted cosmetic tracer from
/// ever becoming an authoritative hit (F57 non-negotiable behavior 3: "A local
/// tracer cannot award a kill").
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct ShotId(u32);

impl ShotId {
    /// Wraps a proposed shot number; zero is refused, as everywhere else in
    /// this crate, so a default field can never name a live shot.
    pub const fn try_new(value: u32) -> Option<Self> {
        if value == 0 { None } else { Some(Self(value)) }
    }

    /// The shot number.
    pub const fn get(self) -> u32 {
        self.0
    }
}

impl fmt::Display for ShotId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "shot {}", self.0)
    }
}

/// What one shooter's shot book remembers: the highest shot ever accepted and
/// the most recent confirmation.
///
/// Constant size, so the deduplication state does not grow with the number of
/// rounds fired (`docs/01-ARCHITECTURE.md`, "consumers keep appropriate
/// deduplication state"). A replay of the most recent confirmation is absorbed
/// by comparing `last`; an *older* shot confirmed after a newer one is a second
/// projectile hitting, which is a second confirmation of a different shot, and
/// the kill it earns is still awarded at most once by the destruction gate.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
struct ShotBook {
    highest: Option<ShotId>,
    last: Option<(ShotId, Tick)>,
    accepted: u32,
    confirmed: u32,
}

/// What the server says about one shot.
///
/// The *only* source of a confirmed hit: `destruction` repeats the existing
/// once-per-generation gate's verdict ([`Destruction::Recorded`] awards the
/// kill, [`Destruction::AlreadyRecorded`] absorbs a second report), so a
/// confirmed hit and an awarded kill cannot come apart.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ShotConfirmation {
    /// Which shot was confirmed.
    pub shot: ShotId,
    /// Who fired it.
    pub shooter: ActorId,
    /// Which generation of that shooter fired it.
    pub generation: ActorGeneration,
    /// The tick the hit was resolved on.
    pub tick: Tick,
    /// The actor that was hit, when the shot hit anything.
    pub target: Option<ActorId>,
    /// What the destruction gate said, when the hit could end an actor.
    pub destruction: Option<Destruction>,
}

/// What one confirmation report did.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ShotOutcome {
    /// The first report for this shot.
    Confirmed(ShotConfirmation),
    /// A replay of the shooter's most recent confirmation. Absorbed: no second
    /// confirmation and no second award, whatever the transport did.
    AlreadyConfirmed {
        /// The replayed shot.
        shot: ShotId,
        /// The tick it was first confirmed on.
        tick: Tick,
    },
}

impl ShotOutcome {
    /// The shot this report is about, whether it was the first report or a
    /// replay.
    #[must_use]
    pub const fn shot(self) -> ShotId {
        match self {
            Self::Confirmed(confirmation) => confirmation.shot,
            Self::AlreadyConfirmed { shot, .. } => shot,
        }
    }

    /// The confirmation, or `None` for a replay.
    #[must_use]
    pub const fn first_confirmation(self) -> Option<ShotConfirmation> {
        match self {
            Self::Confirmed(confirmation) => Some(confirmation),
            Self::AlreadyConfirmed { .. } => None,
        }
    }

    /// Whether this report awarded a kill.
    #[must_use]
    pub const fn awarded(self) -> bool {
        match self {
            Self::Confirmed(ShotConfirmation {
                destruction: Some(Destruction::Recorded { .. }),
                ..
            }) => true,
            Self::Confirmed(_) | Self::AlreadyConfirmed { .. } => false,
        }
    }
}

/// The per-session authority for everything a snapshot publishes.
///
/// A ledger is created for one [`SessionId`] and is never reused across a
/// session generation: a mission retry or a reconnecting host creates a new
/// ledger, so prior generations, destruction records and input acknowledgments
/// never carry over (`docs/01-ARCHITECTURE.md`, "A mission retry creates a
/// fresh session generation").
#[derive(Clone, Debug)]
pub struct NetStateLedger {
    session: SessionId,
    actors: BTreeMap<ActorId, ActorGeneration>,
    states: BTreeMap<ActorId, NetActorState>,
    destruction: BTreeMap<ActorId, Tick>,
    /// One constant-size shot book per actor (F57-C). Dropped with the actor by
    /// [`Self::forget`], so a long session never accumulates shot state for
    /// actors the ledger no longer owns.
    shots: BTreeMap<ActorId, ShotBook>,
    next_generation: u16,
    acknowledged: u32,
}

impl NetStateLedger {
    /// A ledger for `session`. Actor serials are allocated elsewhere
    /// ([`cs_types::net::ActorAllocator`]); the first generation a ledger hands
    /// out is 1, so generation 0 is never a live actor's.
    #[must_use]
    pub fn new(session: SessionId) -> Self {
        Self {
            session,
            actors: BTreeMap::new(),
            states: BTreeMap::new(),
            destruction: BTreeMap::new(),
            shots: BTreeMap::new(),
            next_generation: 1,
            acknowledged: 0,
        }
    }

    /// The session this ledger owns.
    #[must_use]
    pub const fn session(&self) -> SessionId {
        self.session
    }

    /// Registers a newly spawned actor at generation 1.
    ///
    /// # Errors
    ///
    /// [`NetStateError::ForeignSession`] for an actor of another session,
    /// `InvalidActorId` for a zero session or serial, `DuplicateActor` when the
    /// id is already registered, and `GenerationExhausted` when the generation
    /// space is used up.
    pub fn spawn(&mut self, state: NetActorState) -> Result<ActorGeneration, NetStateError> {
        self.expect_own_actor(state.actor)?;
        if self.actors.contains_key(&state.actor) {
            return Err(NetStateError::DuplicateActor { actor: state.actor });
        }
        let generation = ActorGeneration::try_new(self.next_generation)
            .ok_or(NetStateError::GenerationExhausted)?;
        self.next_generation = self
            .next_generation
            .checked_add(1)
            .ok_or(NetStateError::GenerationExhausted)?;
        let state = NetActorState {
            generation,
            ..state
        };
        state.validate()?;
        self.actors.insert(state.actor, generation);
        self.states.insert(state.actor, state);
        Ok(generation)
    }

    /// Publishes new authoritative state for a live actor.
    ///
    /// The caller must present the generation the ledger owns: this is the check
    /// that keeps a stale packet or a reused buffer from writing into a live
    /// actor's state (`docs/contracts/FLIGHT-PHYSICS.md`, "Commands belong to
    /// one simulation tick and actor generation").
    ///
    /// # Errors
    ///
    /// [`NetStateError::UnknownActor`], `StaleGeneration`, `AlreadyTerminal`,
    /// `ForeignSession` or `OutOfRange` (via [`NetActorState::validate`]).
    pub fn publish(&mut self, state: NetActorState) -> Result<(), NetStateError> {
        self.expect_own_actor(state.actor)?;
        let owned = *self
            .actors
            .get(&state.actor)
            .ok_or(NetStateError::UnknownActor { actor: state.actor })?;
        if state.generation != owned {
            return Err(NetStateError::StaleGeneration {
                actor: state.actor,
                presented: state.generation,
                owned,
            });
        }
        let current = self
            .states
            .get(&state.actor)
            .ok_or(NetStateError::UnknownActor { actor: state.actor })?;
        if current.lifecycle.is_terminal() {
            return Err(NetStateError::AlreadyTerminal {
                actor: state.actor,
                generation: owned,
                lifecycle: current.lifecycle,
            });
        }
        state.validate()?;
        let lifecycle = state.lifecycle;
        self.states.insert(state.actor, state);
        if lifecycle == NetLifecycle::Destroyed && !self.destruction.contains_key(&state.actor) {
            // The tick is not known here; `record_destruction` is the awarding
            // entry point and fills it in. A state that only *reports*
            // destruction still retires the actor.
            self.destruction.insert(state.actor, Tick(0));
        }
        Ok(())
    }

    /// Records a destruction and reports whether it awarded the kill.
    ///
    /// The first report of an `(actor, generation)` moves the actor to
    /// [`NetLifecycle::Destroyed`] and returns [`Destruction::Recorded`]. Every
    /// later report — from a second resolver batch, a retransmitted snapshot,
    /// a reconnect replaying its requests — returns
    /// [`Destruction::AlreadyRecorded`] and changes nothing. That is the
    /// guarantee F57 AC01 asks for, and it is a property of this ledger, not of
    /// how many packets arrived.
    ///
    /// # Errors
    ///
    /// [`NetStateError::UnknownActor`], `StaleGeneration` when the caller
    /// presents a generation the ledger does not own, `ForeignSession`, and
    /// `AlreadyTerminal` when the actor already ended as something other than a
    /// destruction (a bailout or despawn is not a kill).
    pub fn record_destruction(
        &mut self,
        actor: ActorId,
        generation: ActorGeneration,
        tick: Tick,
    ) -> Result<Destruction, NetStateError> {
        self.expect_own_actor(actor)?;
        let owned = *self
            .actors
            .get(&actor)
            .ok_or(NetStateError::UnknownActor { actor })?;
        if generation != owned {
            return Err(NetStateError::StaleGeneration {
                actor,
                presented: generation,
                owned,
            });
        }
        if let Some(first) = self.destruction.get(&actor) {
            return Ok(Destruction::AlreadyRecorded { tick: *first });
        }
        let current = self
            .states
            .get(&actor)
            .ok_or(NetStateError::UnknownActor { actor })?;
        if current.lifecycle.is_terminal() {
            return Err(NetStateError::AlreadyTerminal {
                actor,
                generation: owned,
                lifecycle: current.lifecycle,
            });
        }
        self.destruction.insert(actor, tick);
        if let Some(state) = self.states.get_mut(&actor) {
            state.lifecycle = NetLifecycle::Destroyed;
        }
        Ok(Destruction::Recorded { tick })
    }

    /// Ends an actor's record without a kill: bailout, mission removal or
    /// capture.
    ///
    /// Idempotent in the sense that matters: the *first* terminal lifecycle for
    /// a generation is the one kept, so a later report cannot turn a bailout
    /// into a destruction and award a kill that never happened.
    ///
    /// # Errors
    ///
    /// [`NetStateError::UnknownActor`], `StaleGeneration`, `ForeignSession`, and
    /// `AlreadyTerminal` when the actor already ended.
    pub fn end_lifecycle(
        &mut self,
        actor: ActorId,
        generation: ActorGeneration,
        lifecycle: NetLifecycle,
    ) -> Result<(), NetStateError> {
        if lifecycle == NetLifecycle::Alive {
            return Err(NetStateError::OutOfRange { field: "lifecycle" });
        }
        self.expect_own_actor(actor)?;
        let owned = *self
            .actors
            .get(&actor)
            .ok_or(NetStateError::UnknownActor { actor })?;
        if generation != owned {
            return Err(NetStateError::StaleGeneration {
                actor,
                presented: generation,
                owned,
            });
        }
        let current = self
            .states
            .get(&actor)
            .ok_or(NetStateError::UnknownActor { actor })?;
        if current.lifecycle.is_terminal() {
            return Err(NetStateError::AlreadyTerminal {
                actor,
                generation: owned,
                lifecycle: current.lifecycle,
            });
        }
        if let Some(state) = self.states.get_mut(&actor) {
            state.lifecycle = lifecycle;
        }
        Ok(())
    }

    /// Accepts one fire intent from `shooter` as `shot`.
    ///
    /// Weapon acceptance is the server's (`docs/contracts/UI-NETWORK.md`
    /// ownership table), so this is where a proposed shot number becomes a
    /// shot: the client proposes it with its local input, and only a shot that
    /// passes here can ever be named by a confirmation. The shot must be
    /// strictly newer than every shot already accepted for the shooter, so a
    /// replayed or reordered fire intent is refused instead of firing twice.
    ///
    /// The number is the shooter's own, not a server counter, so a client is
    /// free to skip numbers its own firing model never produced. Nothing about
    /// the shot is published as state: ammunition stays where the weapon domain
    /// owns it ([`NetWeapons`] is only ever written through
    /// [`Self::publish`]), so a *predicted* shot still cannot spend a round here.
    ///
    /// # Errors
    ///
    /// [`NetStateError::UnknownActor`], `StaleGeneration`, `ForeignSession`,
    /// `InvalidActorId`, `AlreadyTerminal` when the shooter has already ended,
    /// and `StaleShot` for a proposal that is not newer than an accepted one.
    pub fn accept_shot(
        &mut self,
        shooter: ActorId,
        generation: ActorGeneration,
        shot: ShotId,
    ) -> Result<(), NetStateError> {
        self.expect_own_actor(shooter)?;
        let owned = *self
            .actors
            .get(&shooter)
            .ok_or(NetStateError::UnknownActor { actor: shooter })?;
        if generation != owned {
            return Err(NetStateError::StaleGeneration {
                actor: shooter,
                presented: generation,
                owned,
            });
        }
        let current = self
            .states
            .get(&shooter)
            .ok_or(NetStateError::UnknownActor { actor: shooter })?;
        if current.lifecycle.is_terminal() {
            return Err(NetStateError::AlreadyTerminal {
                actor: shooter,
                generation: owned,
                lifecycle: current.lifecycle,
            });
        }
        let book = self.shots.entry(shooter).or_default();
        if let Some(highest) = book.highest
            && shot <= highest
        {
            return Err(NetStateError::StaleShot {
                actor: shooter,
                presented: shot,
                highest,
            });
        }
        book.highest = Some(shot);
        book.accepted = book.accepted.saturating_add(1);
        Ok(())
    }

    /// The highest shot this ledger has accepted for `shooter`.
    #[must_use]
    pub fn highest_shot(&self, shooter: ActorId) -> Option<ShotId> {
        self.shots.get(&shooter).and_then(|book| book.highest)
    }

    /// How many shots this ledger has accepted for `shooter`.
    #[must_use]
    pub fn accepted_shots(&self, shooter: ActorId) -> u32 {
        self.shots.get(&shooter).map_or(0, |book| book.accepted)
    }

    /// Confirms that `shot`, fired by `shooter`, hit `target` on `tick`.
    ///
    /// A confirmation is only ever about a shot this ledger accepted, so a
    /// purely predicted cosmetic — one the client drew and the server never
    /// accepted — has nothing to be confirmed by. The first report returns
    /// [`ShotOutcome::Confirmed`]; a replay of the shooter's most recent
    /// confirmation returns [`ShotOutcome::AlreadyConfirmed`] and changes
    /// nothing, which is what makes a retransmitted reliable hit report
    /// idempotent.
    ///
    /// A hit that could end the target is routed through the same
    /// once-per-generation gate as every other destruction, so two shots
    /// hitting the same actor award one kill, and the confirmation reports
    /// which it was. A target that had already ended *without* a kill (a
    /// bailout, a mission removal) awards nothing and is not an error: the hit
    /// happened, the actor was already gone.
    ///
    /// # Errors
    ///
    /// [`NetStateError::UnknownActor`], `ForeignSession` and `InvalidActorId`
    /// for an actor id the ledger does not own, `StaleGeneration` when the
    /// caller presents a generation the ledger does not own, and
    /// `ShotNotAccepted` for a shot that was never accepted. A refused report
    /// changes nothing.
    pub fn confirm_shot(
        &mut self,
        shooter: ActorId,
        generation: ActorGeneration,
        shot: ShotId,
        target: Option<ActorId>,
        tick: Tick,
    ) -> Result<ShotOutcome, NetStateError> {
        self.expect_own_actor(shooter)?;
        let owned = *self
            .actors
            .get(&shooter)
            .ok_or(NetStateError::UnknownActor { actor: shooter })?;
        if generation != owned {
            return Err(NetStateError::StaleGeneration {
                actor: shooter,
                presented: generation,
                owned,
            });
        }
        let book = *self
            .shots
            .get(&shooter)
            .ok_or(NetStateError::ShotNotAccepted {
                actor: shooter,
                shot,
            })?;
        if book.highest.is_none_or(|highest| shot > highest) {
            return Err(NetStateError::ShotNotAccepted {
                actor: shooter,
                shot,
            });
        }
        if let Some((last, last_tick)) = book.last
            && last == shot
        {
            return Ok(ShotOutcome::AlreadyConfirmed {
                shot,
                tick: last_tick,
            });
        }
        // Resolve the target before mutating anything, so a refused target
        // leaves the shot book exactly as it was.
        let resolved = match target {
            None => None,
            Some(target) => {
                self.expect_own_actor(target)?;
                let generation = *self
                    .actors
                    .get(&target)
                    .ok_or(NetStateError::UnknownActor { actor: target })?;
                Some((target, generation))
            }
        };
        let destruction = match resolved {
            None => None,
            Some((target, generation)) => {
                // A target that already ended *without* a kill (a bailout, a
                // mission removal) cannot be routed through the gate and awards
                // nothing: the hit happened, the actor was already gone.
                let live = self
                    .states
                    .get(&target)
                    .is_some_and(|state| !state.lifecycle.is_terminal());
                live.then(|| self.record_destruction(target, generation, tick))
                    .transpose()?
            }
        };
        let book = self.shots.entry(shooter).or_default();
        book.last = Some((shot, tick));
        book.confirmed = book.confirmed.saturating_add(1);
        Ok(ShotOutcome::Confirmed(ShotConfirmation {
            shot,
            shooter,
            generation,
            tick,
            target,
            destruction,
        }))
    }

    /// How many shots this ledger has confirmed for `shooter`.
    #[must_use]
    pub fn confirmed_shots(&self, shooter: ActorId) -> u32 {
        self.shots.get(&shooter).map_or(0, |book| book.confirmed)
    }

    /// Consumes one client input sequence and returns the acknowledgment.
    ///
    /// This is the only client-derived input the ledger accepts, and it changes
    /// exactly one thing: the highest acknowledged sequence. It cannot consume a
    /// round, spend boost capacity or move an actor, so a client's prediction of
    /// a shot or a boost cannot alter authoritative state (F57 AC02).
    ///
    /// A sequence at or below the current one is refused as
    /// [`NetStateError::StaleInput`]: transport-level duplicate suppression
    /// (`docs/contracts/UI-NETWORK.md`) is a courtesy, and this is the rule.
    ///
    /// # Errors
    ///
    /// [`NetStateError::StaleInput`] for a replayed or reordered sequence.
    pub fn observe_input(&mut self, sequence: u32, tick: Tick) -> Result<InputAck, NetStateError> {
        if sequence <= self.acknowledged {
            return Err(NetStateError::StaleInput {
                presented: sequence,
                acknowledged: self.acknowledged,
            });
        }
        self.acknowledged = sequence;
        Ok(InputAck { sequence, tick })
    }

    /// The highest client input sequence consumed.
    #[must_use]
    pub const fn acknowledged_input(&self) -> u32 {
        self.acknowledged
    }

    /// The authoritative state of `actor`, if the ledger knows it.
    #[must_use]
    pub fn state(&self, actor: ActorId) -> Option<&NetActorState> {
        self.states.get(&actor)
    }

    /// Every authoritative state, in `ActorId` order — the set a snapshot for
    /// this tick publishes.
    pub fn states(&self) -> impl Iterator<Item = &NetActorState> {
        self.states.values()
    }

    /// How many actors the ledger tracks.
    #[must_use]
    pub fn actor_count(&self) -> usize {
        self.states.len()
    }

    /// The generation the ledger owns for `actor`.
    ///
    /// # Errors
    ///
    /// [`NetStateError::UnknownActor`] when the ledger does not know the actor.
    pub fn generation(&self, actor: ActorId) -> Result<ActorGeneration, NetStateError> {
        self.actors
            .get(&actor)
            .copied()
            .ok_or(NetStateError::UnknownActor { actor })
    }

    /// Whether the actor's destruction has already been recorded.
    #[must_use]
    pub fn destruction_recorded(&self, actor: ActorId) -> bool {
        self.destruction.contains_key(&actor)
    }

    /// Drops an actor and its deduplication state entirely.
    ///
    /// This is how the retention stays bounded: the destruction record lives
    /// exactly as long as the actor it belongs to, and a session that has
    /// forgotten an actor cannot award a second kill for it (the id is gone, so
    /// a later report is [`NetStateError::UnknownActor`]).
    ///
    /// # Errors
    ///
    /// [`NetStateError::UnknownActor`] when the ledger does not know the actor.
    pub fn forget(&mut self, actor: ActorId) -> Result<(), NetStateError> {
        self.expect_own_actor(actor)?;
        self.actors
            .remove(&actor)
            .ok_or(NetStateError::UnknownActor { actor })?;
        self.states.remove(&actor);
        self.destruction.remove(&actor);
        self.shots.remove(&actor);
        Ok(())
    }

    /// The event identity a published snapshot of this tick would carry, for
    /// `producer` at `sequence`.
    ///
    /// Snapshots are sequenced and droppable, so they are not events; this is
    /// the identity a *reliable* publication of the same tick's fact (an actor
    /// removal, a lifecycle change) would use, kept here so a caller cannot
    /// invent one that names another session.
    #[must_use]
    pub fn event_id(&self, tick: Tick, producer: u32, sequence: u32) -> EventId {
        EventId {
            session: self.session,
            tick,
            producer,
            sequence,
        }
    }

    /// Checks the actor names a live actor id and belongs to this ledger.
    ///
    /// The two refusals are distinct because they are different mistakes: a zero
    /// session or serial is a malformed id, while a nonzero id of another session
    /// generation is a valid id that belongs to a session this ledger does not own.
    fn expect_own_actor(&self, actor: ActorId) -> Result<(), NetStateError> {
        if actor.session.get() == 0 || actor.serial == 0 {
            return Err(NetStateError::InvalidActorId { actor });
        }
        if actor.session != self.session {
            return Err(NetStateError::ForeignSession {
                expected: self.session,
                found: actor.session,
            });
        }
        Ok(())
    }
}

impl NetActorState {
    /// A live actor at a world pose, with no velocity, full flight, damage and
    /// weapon state.
    ///
    /// The ergonomic constructor for a spawn: `position` and `orientation` are
    /// the only inputs a caller usually has.
    #[must_use]
    pub fn spawn(
        actor: ActorId,
        position: WorldPosition,
        orientation: Quaternion,
        rounds_per_bank: u32,
    ) -> Self {
        Self {
            actor,
            // Placeholder: `NetStateLedger::spawn` overwrites this with the
            // generation it allocates. It is nonzero so the value is never
            // meaningless even before the ledger hands it over.
            generation: ActorGeneration::try_new(1).expect("one is a valid generation"),
            lifecycle: NetLifecycle::Alive,
            control: NetControlMode::Manual,
            pose: NetPose {
                position,
                orientation,
            },
            linear_velocity_mps: [0.0; 3],
            angular_velocity_radps: [0.0; 3],
            flight: NetFlight {
                throttle: 0.0,
                engine_spool: 0.0,
                boost_capacity: 1.0,
            },
            damage: NetDamage {
                remaining: 1.0,
                disabled_mounts: 0,
            },
            weapons: NetWeapons {
                primary_rounds: rounds_per_bank,
                secondary_rounds: rounds_per_bank,
                selected_bank: 0,
            },
        }
    }

    /// Rejects non-finite input and out-of-range fractions.
    ///
    /// The contract's rule ("Reject nonfinite inputs at boundaries") applied at
    /// the boundary that produces authoritative state, so nothing non-finite ever
    /// reaches a quantizer or a snapshot budget.
    ///
    /// # Errors
    ///
    /// [`NetStateError::OutOfRange`] naming the offending field.
    pub fn validate(&self) -> Result<(), NetStateError> {
        const VELOCITY_FIELDS: [&str; 3] = [
            "state.linear_velocity_mps[0]",
            "state.linear_velocity_mps[1]",
            "state.linear_velocity_mps[2]",
        ];
        const ANGULAR_FIELDS: [&str; 3] = [
            "state.angular_velocity_radps[0]",
            "state.angular_velocity_radps[1]",
            "state.angular_velocity_radps[2]",
        ];
        if self.generation.get() == 0 {
            return Err(NetStateError::OutOfRange {
                field: "state.generation",
            });
        }
        for (field, value) in VELOCITY_FIELDS.into_iter().zip(self.linear_velocity_mps) {
            if !value.is_finite() {
                return Err(NetStateError::OutOfRange { field });
            }
        }
        for (field, value) in ANGULAR_FIELDS.into_iter().zip(self.angular_velocity_radps) {
            if !value.is_finite() {
                return Err(NetStateError::OutOfRange { field });
            }
        }
        for (field, value) in [
            ("state.flight.throttle", self.flight.throttle),
            ("state.flight.engine_spool", self.flight.engine_spool),
            ("state.flight.boost_capacity", self.flight.boost_capacity),
        ] {
            if !value.is_finite() || !(0.0..=1.0).contains(&value) {
                return Err(NetStateError::OutOfRange { field });
            }
        }
        if !self.damage.remaining.is_finite() || !(0.0..=1.0).contains(&self.damage.remaining) {
            return Err(NetStateError::OutOfRange {
                field: "state.damage.remaining",
            });
        }
        if self.weapons.primary_rounds > NetWeapons::MAX_ROUNDS
            || self.weapons.secondary_rounds > NetWeapons::MAX_ROUNDS
        {
            return Err(NetStateError::OutOfRange {
                field: "state.weapons.rounds",
            });
        }
        if !NetWeapons::BANK_CODES.contains(&self.weapons.selected_bank) {
            return Err(NetStateError::OutOfRange {
                field: "state.weapons.selected_bank",
            });
        }
        Ok(())
    }
}
