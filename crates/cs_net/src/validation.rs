//! The threat model and the session identity gate (F58-A).
//!
//! Spec: `specs/F58-network-abuse-resistance-reconnect-and-match-recovery.md`,
//! stage `### F58-A`. Contract: `docs/contracts/UI-NETWORK.md` ("Wire ids are
//! stable typed numeric/string keys with bounded lengths. Protocol messages
//! carry session epoch and sequence/tick. Epoch mismatch rejects stale
//! packets." and "Reliable delivery does not replace application idempotency
//! because reconnect/retry can replay requests").
//!
//! # What this stage is
//!
//! F58-A *defines* the threats and the identity rules; it does not implement a
//! whole runtime. This module is the pure host-side gate every decoded
//! [`ClientMessage`] passes before its payload is trusted:
//!
//! * [`ThreatCase`] enumerates every class of stale or hostile traffic the
//!   feature must resist, and [`ThreatDisposition`] says whether the session
//!   absorbs it (a normal replay after a lossy link or a reconnect),
//!   disconnects the peer with a bounded reason (abuse), or can never express
//!   it on the wire at all (the type system is the barrier).
//! * [`SessionIdentity`] is the live session epoch. [`SessionGate`] pairs it
//!   with a per-peer replay window ([`SequenceWindow`]) and the server-owned
//!   peer-to-actor binding ([`ActorOwnership`]).
//! * [`SessionGate::admit`] refuses a packet from an unauthenticated peer, a
//!   packet stamped with a dead epoch, a packet outside the wire bounds, and a
//!   replayed sequence, each with a [`SessionViolation`] that names the live
//!   value it contradicted. A refusal changes no state: the Epoch mismatch does
//!   not advance the replay window, and a replay does not re-apply.
//! * [`FireRequest`] is the typed output weapon acceptance consumes; a packet
//!   the gate refuses yields none, which is how "replay a prior-session fire
//!   packet and prove no projectile spawns" holds at the identity boundary.
//!   The `cs_app::network::recovery` receiver turns an admitted input packet
//!   into these requests, and `cs_sim::weapons::FireResolver` independently
//!   refuses a foreign-session intent, so the property holds at both layers.
//!
//! # What F58-B adds
//!
//! F58-B is the intent layer built on that gate: [`ClientIntent`] is every
//! thing a client may *ask* the server to do, and [`IntentValidator`] judges
//! each ask against the server-owned facts of `docs/contracts/UI-NETWORK.md`
//! — actors ([`ActorOwnership`]), loadouts, rates, tick windows and match
//! state:
//!
//! * [`ClientIntent::ClaimDamage`], [`ClientIntent::ClaimScore`],
//!   [`ClientIntent::ClaimHealth`], [`ClientIntent::ClaimFaction`] and
//!   [`ClientIntent::ClaimOutcome`] author a server-owned domain, so
//!   [`IntentRefusal::ClientAuthoredTruth`] refuses them **before** anything
//!   else runs and no state moves. That is F58's minimum acceptance scenario:
//!   *a client requests damage/score directly; the server rejects it.*
//! * [`IntentValidator`] charges every examined intent to a per-peer
//!   [`RateBudget`] measured in *server* ticks ([`RateLimits`]); a peer over
//!   the design is [`IntentRefusal::RateExceeded`] and a budget asked to
//!   track more peers than [`MAX_SESSION_PEERS`] is
//!   [`IntentRefusal::TooManyPeers`] — the two runtime guards behind
//!   [`ThreatCase::ImpossibleRate`] and [`ThreatCase::ResourceExhaustion`],
//!   which F58-A declared but did not implement.
//! * [`MatchStage`] is where the server's own clock, its phase and the host's
//!   banned components live; input ticks outside the acceptance window and
//!   loadouts that break the host's rules are refused with a bounded reason.
//!
//! A refusal changes nothing: the budget is not charged for a forged-truth
//! ask (that peer is disconnected on the spot), the ownership table is not
//! touched, and a refused intent produces no [`FireRequest`].
//!
//! The disconnect/recovery flow (F58-C) is deliberately absent: F58-B
//! implements the validation and its caps, and wiring them to the pinned
//! transport's pump is the next stage.
//!
//! # Identity rules
//!
//! 1. One live [`SessionId`] epoch. Every in-session packet names it; a packet
//!    naming any other value is stale and changes nothing.
//! 2. Within an epoch, each peer's packet sequence is strictly increasing. A
//!    sequence at or below the highest already admitted is a replay and is
//!    absorbed without application.
//! 3. A peer acts only for the actor the server bound to it
//!    ([`ActorOwnership`]); the client never names its own ownership.
//! 4. Reconnect is a fresh epoch (see [`crate::recovery`]), so rule 1 makes
//!    every packet from the prior connection stale even when its sequence is
//!    new.
//!
//! All values here are newly authored engine design: no original network
//! budget, rate, or abuse behavior has been measured, and none is asserted.

use std::collections::{BTreeMap, BTreeSet};
use std::fmt;

use cs_types::Tick;
use cs_types::content::ContentId;
use cs_types::input::{Action, FlightCommand, InputFrame};
use cs_types::net::{ActorId, PeerId, SessionId};

use crate::authority::AuthorityDomain;
use crate::bounds::MAX_SESSION_PEERS;
use crate::lobby::{
    Loadout, LoadoutProblem, MAX_LOADOUT_COMPONENTS, Phase, TeamId, is_component_kind,
};
use crate::message::{ClientMessage, ClientPayload, InputBatch, MessageHeader, WireError};

/// One class of stale or hostile traffic the session identity rules resist.
///
/// The list is the F58 threat model, not a claim about the original game's
/// network: every entry names behavior the *new* engine must survive, and
/// [`ThreatCase::disposition`] says where this stage stops it.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum ThreatCase {
    /// A packet stamped with a session epoch that is no longer live.
    StaleSession,
    /// A request whose sequence was already admitted (a retry or a replay).
    ReplayedRequest,
    /// An actor-naming request for an aircraft the peer does not own.
    InvalidOwnership,
    /// More requests per tick window than the session designed for.
    ImpossibleRate,
    /// A packet larger than the wire caps allow.
    OversizedMessage,
    /// Traffic that would grow server state without bound.
    ResourceExhaustion,
    /// A client attempting to originate a server-owned payload.
    ForgedServerAuthority,
    /// A client attempting to author server-owned state (score, health, ...).
    ClientAuthoredTruth,
    /// A pick-up or capture reward delivered a second time.
    ReplayedReward,
    /// A reconnect or late join binding to another pilot's aircraft.
    ForeignAircraftBinding,
    /// A packet from a peer the session never admitted.
    UnauthenticatedPeer,
    /// A structurally malformed packet that is not merely oversized.
    MalformedMessage,
}

impl ThreatCase {
    /// Every threat case, in a stable order.
    pub const ALL: &'static [ThreatCase] = &[
        Self::StaleSession,
        Self::ReplayedRequest,
        Self::InvalidOwnership,
        Self::ImpossibleRate,
        Self::OversizedMessage,
        Self::ResourceExhaustion,
        Self::ForgedServerAuthority,
        Self::ClientAuthoredTruth,
        Self::ReplayedReward,
        Self::ForeignAircraftBinding,
        Self::UnauthenticatedPeer,
        Self::MalformedMessage,
    ];

    /// The stable label used in reports.
    pub const fn label(self) -> &'static str {
        match self {
            Self::StaleSession => "stale_session",
            Self::ReplayedRequest => "replayed_request",
            Self::InvalidOwnership => "invalid_ownership",
            Self::ImpossibleRate => "impossible_rate",
            Self::OversizedMessage => "oversized_message",
            Self::ResourceExhaustion => "resource_exhaustion",
            Self::ForgedServerAuthority => "forged_server_authority",
            Self::ClientAuthoredTruth => "client_authored_truth",
            Self::ReplayedReward => "replayed_reward",
            Self::ForeignAircraftBinding => "foreign_aircraft_binding",
            Self::UnauthenticatedPeer => "unauthenticated_peer",
            Self::MalformedMessage => "malformed_message",
        }
    }

    /// What the session does about this threat.
    pub const fn disposition(self) -> ThreatDisposition {
        match self {
            // Normal on a lossy link or after a reconnect: refuse, change
            // nothing, keep the connection.
            Self::StaleSession
            | Self::ReplayedRequest
            | Self::ReplayedReward
            | Self::MalformedMessage => ThreatDisposition::Absorb,
            // Abuse: refuse and cut the peer off with a bounded reason.
            Self::InvalidOwnership
            | Self::ImpossibleRate
            | Self::OversizedMessage
            | Self::ResourceExhaustion
            | Self::ForeignAircraftBinding
            | Self::UnauthenticatedPeer => ThreatDisposition::Disconnect,
            // The wire vocabulary cannot express these at all, so no runtime
            // check is needed (F54-A `message` / `authority`).
            Self::ForgedServerAuthority | Self::ClientAuthoredTruth => {
                ThreatDisposition::Structural
            }
        }
    }
}

impl fmt::Display for ThreatCase {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.label())
    }
}

/// What the session identity rules do about one [`ThreatCase`].
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum ThreatDisposition {
    /// Refuse and change nothing; the connection continues. Replays and
    /// stale-epoch packets are normal, so the server must absorb them.
    Absorb,
    /// Refuse and disconnect the peer with the bounded reason carried by its
    /// [`SessionViolation`].
    Disconnect,
    /// The payload cannot be expressed on the wire; the type system, not a
    /// runtime check, is the barrier.
    Structural,
}

/// Why the gate refused one packet: the bounded reason an absorb or a
/// disconnect reports.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum SessionViolation {
    /// The packet's session epoch is not the live one.
    StaleSession {
        /// The live session.
        expected: SessionId,
        /// The epoch the packet carried.
        found: SessionId,
    },
    /// The packet's sequence is not newer than the highest admitted from this
    /// peer.
    ReplayedSequence {
        /// The sender.
        peer: PeerId,
        /// The sequence the packet carried.
        presented: u32,
        /// The highest sequence already admitted from this peer.
        admitted: u32,
    },
    /// The packet came from a peer this session never admitted.
    UnauthenticatedPeer {
        /// The claimed sender.
        peer: PeerId,
    },
    /// The peer tried to act for an aircraft bound to a different peer.
    ActorNotOwned {
        /// The acting peer.
        peer: PeerId,
        /// The named actor.
        actor: ActorId,
        /// Its current owner, if any.
        owner: Option<PeerId>,
    },
    /// The peer named an actor that has no binding at all.
    UnknownActor {
        /// The acting peer.
        peer: PeerId,
        /// The named actor.
        actor: ActorId,
    },
    /// The packet failed its own wire-bounds validation.
    Malformed(WireError),
}

impl SessionViolation {
    /// The threat class this refusal belongs to.
    pub fn threat(&self) -> ThreatCase {
        match self {
            Self::StaleSession { .. } => ThreatCase::StaleSession,
            Self::ReplayedSequence { .. } => ThreatCase::ReplayedRequest,
            Self::UnauthenticatedPeer { .. } => ThreatCase::UnauthenticatedPeer,
            Self::ActorNotOwned { .. } | Self::UnknownActor { .. } => ThreatCase::InvalidOwnership,
            Self::Malformed(WireError::TooLarge { .. } | WireError::TooMany { .. }) => {
                ThreatCase::OversizedMessage
            }
            Self::Malformed(_) => ThreatCase::MalformedMessage,
        }
    }

    /// What the session does about this refusal.
    pub fn disposition(&self) -> ThreatDisposition {
        self.threat().disposition()
    }

    /// The stable label used in reports.
    pub fn label(&self) -> &'static str {
        self.threat().label()
    }
}

impl fmt::Display for SessionViolation {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::StaleSession { expected, found } => write!(
                f,
                "packet names {found}, but the live session is {expected}"
            ),
            Self::ReplayedSequence {
                peer,
                presented,
                admitted,
            } => write!(
                f,
                "{peer} sequence {presented} is not newer than the admitted {admitted}"
            ),
            Self::UnauthenticatedPeer { peer } => {
                write!(f, "{peer} was never admitted to this session")
            }
            Self::ActorNotOwned { peer, actor, owner } => match owner {
                Some(owner) => write!(f, "{peer} does not own {actor}; it belongs to {owner}"),
                None => write!(f, "{peer} does not own {actor}"),
            },
            Self::UnknownActor { peer, actor } => {
                write!(f, "{peer} named {actor}, which has no owner")
            }
            Self::Malformed(reason) => write!(f, "malformed packet: {reason}"),
        }
    }
}

impl std::error::Error for SessionViolation {}

/// The live session epoch and the pure wire rule that uses it.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct SessionIdentity {
    session: SessionId,
}

impl SessionIdentity {
    /// The live session epoch.
    #[must_use]
    pub const fn new(session: SessionId) -> Self {
        Self { session }
    }

    /// The live session epoch.
    #[must_use]
    pub const fn session(self) -> SessionId {
        self.session
    }

    /// Checks a packet's header against the live epoch.
    ///
    /// This is rule 1 of the module contract and the check that makes a
    /// reconnect safe: the new connection runs a fresh epoch, so every packet
    /// from the old one is stale by construction.
    ///
    /// # Errors
    ///
    /// [`SessionViolation::StaleSession`].
    pub fn check(&self, header: &MessageHeader) -> Result<(), SessionViolation> {
        if header.session == self.session {
            Ok(())
        } else {
            Err(SessionViolation::StaleSession {
                expected: self.session,
                found: header.session,
            })
        }
    }
}

/// The host's replay window for one peer: the highest packet sequence it has
/// admitted.
///
/// The window is a single integer, not a set: input obeys a strict "newer or
/// nothing" rule, so memory is bounded by the peer count regardless of how
/// many packets (or replays) arrive. The first packet from a peer is admitted
/// whatever its sequence; every later one must be strictly greater.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct SequenceWindow {
    admitted: Option<u32>,
}

impl SequenceWindow {
    /// An empty window: no sequence has been admitted yet.
    #[must_use]
    pub const fn new() -> Self {
        Self { admitted: None }
    }

    /// The highest admitted sequence, if the peer has sent anything.
    #[must_use]
    pub const fn admitted(&self) -> Option<u32> {
        self.admitted
    }

    /// Whether `sequence` is newer than everything already admitted.
    #[must_use]
    pub const fn admits(&self, sequence: u32) -> bool {
        match self.admitted {
            Some(admitted) => sequence > admitted,
            None => true,
        }
    }

    /// Admits `sequence`, advancing the window. Returns `false` and changes
    /// nothing for a replay.
    pub const fn admit(&mut self, sequence: u32) -> bool {
        if self.admits(sequence) {
            self.admitted = Some(sequence);
            true
        } else {
            false
        }
    }
}

/// The server-owned binding between peers and the actors they fly.
///
/// The client never names its own ownership (contract: "Server owns ActorId
/// allocation"); this table is the server's record of it. It is deliberately
/// separate from [`SessionGate::admit`] so F58-B's actor-naming intents can
/// call [`ActorOwnership::authorize`] before they are composed, while the
/// session identity gate stays about epochs and sequences.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct ActorOwnership {
    by_actor: BTreeMap<ActorId, PeerId>,
}

impl ActorOwnership {
    /// An empty ownership table.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Binds `actor` to `peer`, idempotently for the same pair.
    ///
    /// # Errors
    ///
    /// [`SessionViolation::ActorNotOwned`] when another peer already owns the
    /// actor. A peer never takes an aircraft from another pilot.
    pub fn bind(&mut self, peer: PeerId, actor: ActorId) -> Result<(), SessionViolation> {
        match self.by_actor.get(&actor) {
            Some(&owner) if owner != peer => Err(SessionViolation::ActorNotOwned {
                peer,
                actor,
                owner: Some(owner),
            }),
            _ => {
                self.by_actor.insert(actor, peer);
                Ok(())
            }
        }
    }

    /// Releases `actor`, returning its former owner.
    pub fn release(&mut self, actor: ActorId) -> Option<PeerId> {
        self.by_actor.remove(&actor)
    }

    /// The peer that owns `actor`, if any.
    #[must_use]
    pub fn owner(&self, actor: ActorId) -> Option<PeerId> {
        self.by_actor.get(&actor).copied()
    }

    /// The actor `peer` owns, if exactly one is bound.
    ///
    /// A peer flies at most one aircraft per session in the designed scope, so
    /// a second binding would be a caller bug; the first bound actor is
    /// returned.
    #[must_use]
    pub fn actor_of(&self, peer: PeerId) -> Option<ActorId> {
        self.by_actor
            .iter()
            .find_map(|(actor, owner)| (*owner == peer).then_some(*actor))
    }

    /// Authorizes `peer` to act for `actor`.
    ///
    /// # Errors
    ///
    /// [`SessionViolation::ActorNotOwned`] when another peer owns it,
    /// [`SessionViolation::UnknownActor`] when nothing does.
    pub fn authorize(&self, peer: PeerId, actor: ActorId) -> Result<(), SessionViolation> {
        match self.by_actor.get(&actor) {
            Some(&owner) if owner == peer => Ok(()),
            Some(&owner) => Err(SessionViolation::ActorNotOwned {
                peer,
                actor,
                owner: Some(owner),
            }),
            None => Err(SessionViolation::UnknownActor { peer, actor }),
        }
    }
}

/// The host's decision about one decoded client packet.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Admission {
    /// Validated; the payload may be applied. `sequence` is now the peer's
    /// highest admitted sequence.
    Accepted {
        /// The admitted sequence.
        sequence: u32,
    },
    /// Refused with a bounded reason. The reason's
    /// [`SessionViolation::disposition`] says whether to absorb it or
    /// disconnect the peer.
    Refused(SessionViolation),
}

impl Admission {
    /// Whether the packet was admitted.
    #[must_use]
    pub const fn accepted(&self) -> bool {
        matches!(self, Self::Accepted { .. })
    }

    /// The refusal, when there was one.
    #[must_use]
    pub const fn violation(&self) -> Option<&SessionViolation> {
        match self {
            Self::Refused(violation) => Some(violation),
            Self::Accepted { .. } => None,
        }
    }
}

/// The per-peer admission state: the epoch, the wire caps and the replay
/// window.
#[derive(Clone, Debug)]
pub struct SessionGate {
    identity: SessionIdentity,
    peers: BTreeMap<PeerId, SequenceWindow>,
    ownership: ActorOwnership,
}

impl SessionGate {
    /// A gate for the live session `session`, with no peers admitted.
    #[must_use]
    pub fn new(session: SessionId) -> Self {
        Self {
            identity: SessionIdentity::new(session),
            peers: BTreeMap::new(),
            ownership: ActorOwnership::new(),
        }
    }

    /// The live session epoch.
    #[must_use]
    pub const fn identity(&self) -> SessionIdentity {
        self.identity
    }

    /// The server-owned peer-to-actor bindings.
    #[must_use]
    pub fn ownership(&self) -> &ActorOwnership {
        &self.ownership
    }

    /// The server-owned peer-to-actor bindings, mutably.
    pub fn ownership_mut(&mut self) -> &mut ActorOwnership {
        &mut self.ownership
    }

    /// Admits `peer` to the session; idempotent.
    pub fn admit_peer(&mut self, peer: PeerId) {
        self.peers.entry(peer).or_default();
    }

    /// Forgets a departed peer entirely, including its replay window.
    ///
    /// The window dies with the peer: a returned pilot is a *new* peer id in
    /// the live epoch (contract: peer ids are never recycled), so keeping the
    /// old window could only suppress a legitimate first packet.
    pub fn forget_peer(&mut self, peer: PeerId) {
        self.peers.remove(&peer);
    }

    /// Whether `peer` was admitted.
    #[must_use]
    pub fn is_member(&self, peer: PeerId) -> bool {
        self.peers.contains_key(&peer)
    }

    /// How many peers are admitted. Bounded by [`crate::bounds::MAX_SESSION_PEERS`].
    #[must_use]
    pub fn peer_count(&self) -> usize {
        self.peers.len()
    }

    /// Admits one packet from `peer`.
    ///
    /// The checks run in this order, and a refusal never changes state:
    ///
    /// 1. membership — an unauthenticated peer is refused before its epoch is
    ///    even read;
    /// 2. the session epoch — a stale packet is absorbed and does **not**
    ///    advance the replay window;
    /// 3. the payload's own wire bounds ([`ClientMessage::validate`]);
    /// 4. the replay window — a sequence at or below the admitted one is
    ///    absorbed.
    ///
    /// # Errors
    ///
    /// Returns [`Admission::Refused`] with a [`SessionViolation`]; this
    /// function never panics.
    pub fn admit(&mut self, peer: PeerId, message: &ClientMessage) -> Admission {
        let Some(window) = self.peers.get_mut(&peer) else {
            return Admission::Refused(SessionViolation::UnauthenticatedPeer { peer });
        };
        if let Err(violation) = self.identity.check(&message.header) {
            return Admission::Refused(violation);
        }
        if let Err(reason) = message.validate() {
            return Admission::Refused(SessionViolation::Malformed(reason));
        }
        let sequence = message.header.sequence;
        let previous = window.admitted().unwrap_or(0);
        if !window.admit(sequence) {
            return Admission::Refused(SessionViolation::ReplayedSequence {
                peer,
                presented: sequence,
                admitted: previous,
            });
        }
        Admission::Accepted { sequence }
    }
}

/// A validated fire request: the typed output weapon acceptance (F27) may
/// consume.
///
/// The only production path that produces one is the app's session receiver
/// ([`crate::validation`]'s consumer in `cs_app::network::recovery`), after
/// [`SessionGate::admit`] has cleared the packet. A refused packet therefore
/// yields no request — no weapon acceptance, and no projectile.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct FireRequest {
    /// The live session the request belongs to.
    pub session: SessionId,
    /// The peer that asked.
    pub peer: PeerId,
    /// The aircraft the server bound to that peer.
    pub actor: ActorId,
    /// The simulation tick the request belongs to.
    pub tick: Tick,
    /// The asking packet's sequence, which orders this peer's requests.
    pub sequence: u32,
}

/// The fire edges one admitted input batch requests, oldest frame first.
///
/// A frame's [`FlightCommand::FirePrimary`] edge is a *request*; the caller
/// turns it into the F27 intent that the authoritative resolver accepts or
/// refuses. This function only extracts; the reply says nothing about whether
/// a shot happens.
#[must_use]
pub fn fire_requests(
    batch: &InputBatch,
    session: SessionId,
    peer: PeerId,
    actor: ActorId,
    sequence: u32,
) -> Vec<FireRequest> {
    let mut requests = Vec::new();
    for frame in &batch.frames {
        for edge in frame.edges() {
            if matches!(edge, Action::Flight(FlightCommand::FirePrimary)) {
                requests.push(FireRequest {
                    session,
                    peer,
                    actor,
                    tick: frame.frame_tick(),
                    sequence,
                });
            }
        }
    }
    requests
}

/// A minimal synthetic in-session fire packet: one frame at `tick` holding a
/// single primary-fire edge, stamped with `session` and `sequence`.
///
/// This is the fixture the F58-A acceptance scenario replays; it is newly
/// authored development content and can never stand in for retail traffic.
#[must_use]
pub fn synthetic_fire_message(session: SessionId, tick: Tick, sequence: u32) -> ClientMessage {
    let mut frame = InputFrame::new(tick);
    frame.push_edge(Action::Flight(FlightCommand::FirePrimary));
    ClientMessage {
        header: MessageHeader { session, sequence },
        payload: ClientPayload::Input(InputBatch {
            frames: vec![frame],
        }),
    }
}

/// A minimal synthetic in-session input packet carrying no fire edge, for
/// callers that only need a bounded valid payload.
#[must_use]
pub fn synthetic_idle_message(session: SessionId, tick: Tick, sequence: u32) -> ClientMessage {
    let mut frame = InputFrame::new(tick);
    frame.set_axis(
        cs_types::input::AxisValue::from_unit(FlightCommand::Throttle, 0.5)
            .expect("half throttle is a finite in-range axis"),
    );
    ClientMessage {
        header: MessageHeader { session, sequence },
        payload: ClientPayload::Input(InputBatch {
            frames: vec![frame],
        }),
    }
}

// ---------------------------------------------------------------------------
// Intent validation, rate caps and resource caps (F58-B)
// ---------------------------------------------------------------------------

/// The tick span input frames may lag the server's own tick before the
/// window refuses them.
///
/// Newly authored engine design at a 60 Hz simulation (about one second of
/// tolerance); no original input-acceptance window has been measured and none
/// is asserted.
pub const MAX_INPUT_TICKS_BEHIND: u64 = 64;

/// The tick span input frames may run ahead of the server's own tick.
///
/// Newly authored engine design, same basis as [`MAX_INPUT_TICKS_BEHIND`].
pub const MAX_INPUT_TICKS_AHEAD: u64 = 64;

/// The rate design the [`IntentValidator`] enforces: at most
/// `max_intents_per_window` intents from one peer per `window_ticks` of the
/// *server's* clock.
///
/// Newly authored engine design: 96 intents over 60 server ticks (one second
/// at 60 Hz) leaves headroom over the honest one-input-packet-per-tick rate
/// while bounding a flooding peer to a fixed count per second. No original
/// network rate has been measured, and none is asserted.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct RateLimits {
    /// How many server ticks one rate window lasts.
    pub window_ticks: u64,
    /// How many intents one peer may have charged in one window.
    pub max_intents_per_window: u32,
}

impl RateLimits {
    /// The designed rate: [`Self::window_ticks`] 60, [`Self::max_intents_per_window`] 96.
    pub const DESIGNED: Self = Self {
        window_ticks: 60,
        max_intents_per_window: 96,
    };
}

impl Default for RateLimits {
    fn default() -> Self {
        Self::DESIGNED
    }
}

/// Why the rate budget refused to charge a peer.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum RateViolation {
    /// The peer already charged `limit` intents in server-tick window
    /// `window`.
    Exceeded {
        /// The designed cap.
        limit: u32,
        /// The window the cap was reached in.
        window: u64,
    },
    /// The budget would have to track more than `max` peers to remember this
    /// one, so it refuses instead of growing.
    TooManyPeers {
        /// [`MAX_SESSION_PEERS`].
        max: usize,
    },
}

impl fmt::Display for RateViolation {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Exceeded { limit, window } => {
                write!(f, "already charged {limit} intents in tick window {window}")
            }
            Self::TooManyPeers { max } => {
                write!(f, "the rate budget may track at most {max} peers")
            }
        }
    }
}

impl std::error::Error for RateViolation {}

/// One peer's charge in one rate window.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct RateCharge {
    /// The server-tick window these charges belong to.
    window: u64,
    /// How many intents were charged in it.
    charged: u32,
}

/// The per-peer rate budget: bounded by [`MAX_SESSION_PEERS`] peers, one
/// counter each, keyed on the **server's** tick.
///
/// A client cannot refill its own budget by naming new ticks: the window
/// comes from `server_tick`, which the host supplies, so a peer that floods
/// stays over the cap until the host's own clock moves the window on. State
/// is `O(peers)`, never `O(packets)`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RateBudget {
    limits: RateLimits,
    peers: BTreeMap<PeerId, RateCharge>,
}

impl RateBudget {
    /// A budget enforcing `limits`.
    #[must_use]
    pub fn new(limits: RateLimits) -> Self {
        Self {
            limits,
            peers: BTreeMap::new(),
        }
    }

    /// A budget enforcing [`RateLimits::DESIGNED`].
    #[must_use]
    pub fn designed() -> Self {
        Self::new(RateLimits::DESIGNED)
    }

    /// The limits this budget enforces.
    #[must_use]
    pub const fn limits(&self) -> RateLimits {
        self.limits
    }

    /// How many peers the budget is currently tracking. Never above
    /// [`MAX_SESSION_PEERS`].
    #[must_use]
    pub fn peer_count(&self) -> usize {
        self.peers.len()
    }

    /// Charges `cost` intents from `peer` against the window `server_tick`
    /// falls in.
    ///
    /// # Errors
    ///
    /// [`RateViolation::Exceeded`] when the window is already at its cap —
    /// the count is left unchanged, so retrying gains nothing.
    /// [`RateViolation::TooManyPeers`] when tracking `peer` would push the
    /// budget past [`MAX_SESSION_PEERS`] — nothing is inserted, so the map
    /// cannot grow without bound either.
    pub fn charge(
        &mut self,
        peer: PeerId,
        server_tick: Tick,
        cost: u32,
    ) -> Result<u32, RateViolation> {
        let window = server_tick.0 / self.limits.window_ticks.max(1);
        let limit = self.limits.max_intents_per_window;
        match self.peers.get_mut(&peer) {
            Some(charge) if charge.window == window => {
                let charged = charge.charged.saturating_add(cost);
                if charged > limit {
                    return Err(RateViolation::Exceeded { limit, window });
                }
                charge.charged = charged;
                Ok(charged)
            }
            Some(charge) => {
                if cost > limit {
                    return Err(RateViolation::Exceeded { limit, window });
                }
                *charge = RateCharge {
                    window,
                    charged: cost,
                };
                Ok(cost)
            }
            None => {
                if self.peers.len() >= MAX_SESSION_PEERS {
                    return Err(RateViolation::TooManyPeers {
                        max: MAX_SESSION_PEERS,
                    });
                }
                if cost > limit {
                    return Err(RateViolation::Exceeded { limit, window });
                }
                self.peers.insert(
                    peer,
                    RateCharge {
                        window,
                        charged: cost,
                    },
                );
                Ok(cost)
            }
        }
    }

    /// Forgets every peer's charge (a fresh session epoch starts empty).
    pub fn reset(&mut self) {
        self.peers.clear();
    }
}

/// The host's live match facts an intent is judged against.
///
/// `server_tick` is the anchor for both the input tick window and the rate
/// windows, so neither can be moved by anything the client sends.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct MatchStage<'a> {
    /// Where the lobby/match is ([`crate::lobby::Phase`]).
    pub phase: Phase,
    /// The server's own simulation tick.
    pub server_tick: Tick,
    /// The host's banned components (F55 [`crate::lobby::LobbyRules::banned`]).
    pub banned_components: &'a BTreeSet<ContentId>,
}

impl<'a> MatchStage<'a> {
    /// A stage at `phase`, `server_tick`, with `banned_components` the host
    /// forbids.
    #[must_use]
    pub const fn new(
        phase: Phase,
        server_tick: Tick,
        banned_components: &'a BTreeSet<ContentId>,
    ) -> Self {
        Self {
            phase,
            server_tick,
            banned_components,
        }
    }
}

/// One thing a client asks the server to do, in the vocabulary the host
/// validates before it trusts any of it.
///
/// The wire vocabulary ([`crate::message::ClientPayload`]) can only *express*
/// [`ClientIntent::Input`]. The `Claim*` variants exist so the server can name
/// and refuse a client-authored request instead of silently trusting it — the
/// same reason [`crate::recovery::ClientClaim`] lists what it refuses — and
/// [`ClientIntent::EquipLoadout`] is the one non-input request a client may
/// legitimately make. Nothing on the wire decodes into a `Claim*` today; if a
/// future producer or a local caller offers one, this is where it dies.
#[derive(Clone, Debug, PartialEq)]
pub enum ClientIntent {
    /// Tick-stamped input for the aircraft this peer flies. `actor` is `None`
    /// when the peer has no aircraft bound yet (join before spawn), which is
    /// the one case where an intent names no actor.
    Input {
        /// The aircraft the peer asks to act for, when it has one.
        actor: Option<ActorId>,
        /// The tick the input belongs to.
        tick: Tick,
    },
    /// Ask to fly `loadout` instead: a request the server judges against the
    /// host's rules, never an applied loadout.
    EquipLoadout {
        /// The aircraft and component set the client asks for.
        loadout: Loadout,
    },
    /// Ask the server to remove `amount` structure from `target`.
    ClaimDamage {
        /// The aircraft the client says it damaged.
        target: ActorId,
        /// The structure it says it removed.
        amount: u32,
    },
    /// Ask the server to award `points` to the asker.
    ClaimScore {
        /// The points it claims.
        points: i64,
    },
    /// Ask the server to set `actor`'s remaining structure.
    ClaimHealth {
        /// The aircraft whose health the client claims.
        actor: ActorId,
        /// The fraction it claims, in `(0, 1]`.
        fraction: f64,
    },
    /// Ask the server to put the asker on `team`.
    ClaimFaction {
        /// The team it claims.
        team: TeamId,
    },
    /// Ask the server to declare the match's outcome.
    ClaimOutcome,
}

impl ClientIntent {
    /// The stable label used in reports.
    pub const fn label(&self) -> &'static str {
        match self {
            Self::Input { .. } => "input",
            Self::EquipLoadout { .. } => "equip_loadout",
            Self::ClaimDamage { .. } => "claim_damage",
            Self::ClaimScore { .. } => "claim_score",
            Self::ClaimHealth { .. } => "claim_health",
            Self::ClaimFaction { .. } => "claim_faction",
            Self::ClaimOutcome => "claim_outcome",
        }
    }

    /// The authority domain this ask touches
    /// ([`crate::authority::AuthorityDomain`]).
    pub const fn domain(&self) -> AuthorityDomain {
        match self {
            Self::Input { .. } => AuthorityDomain::LocalInput,
            Self::EquipLoadout { .. } => AuthorityDomain::LobbyRules,
            Self::ClaimDamage { .. } | Self::ClaimHealth { .. } => AuthorityDomain::HitDamage,
            Self::ClaimScore { .. } => AuthorityDomain::ScoreResult,
            Self::ClaimFaction { .. } => AuthorityDomain::FactionInteraction,
            Self::ClaimOutcome => AuthorityDomain::MissionProgram,
        }
    }

    /// Whether the ask *authors* server-owned truth rather than requesting
    /// something the server decides. Every `Claim*` does, and
    /// [`IntentRefusal::ClientAuthoredTruth`] refuses exactly these before any
    /// other check runs.
    pub const fn asserts_truth(&self) -> bool {
        matches!(
            self,
            Self::ClaimDamage { .. }
                | Self::ClaimScore { .. }
                | Self::ClaimHealth { .. }
                | Self::ClaimFaction { .. }
                | Self::ClaimOutcome
        )
    }

    /// The actor this ask names, when it names one.
    #[must_use]
    pub fn actor(&self) -> Option<ActorId> {
        match self {
            Self::Input { actor, .. } => *actor,
            Self::ClaimDamage { target, .. } => Some(*target),
            Self::ClaimHealth { actor, .. } => Some(*actor),
            Self::EquipLoadout { .. }
            | Self::ClaimScore { .. }
            | Self::ClaimFaction { .. }
            | Self::ClaimOutcome => None,
        }
    }
}

impl fmt::Display for ClientIntent {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.label())
    }
}

/// Why the server refused one client intent: the bounded reason an intent
/// layer refusal reports.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum IntentRefusal {
    /// The ask authors a server-owned domain (damage, score, health, faction
    /// or outcome).
    ClientAuthoredTruth {
        /// The domain the client tried to write.
        domain: AuthorityDomain,
    },
    /// The ask names an aircraft this peer does not own.
    NotOwned(SessionViolation),
    /// The asked-for loadout breaks the host's rules.
    Loadout(LoadoutProblem),
    /// The input tick sits outside the acceptance window around the server's
    /// own tick.
    TickOutsideWindow {
        /// The tick the client presented.
        tick: Tick,
        /// The oldest accepted tick.
        earliest: Tick,
        /// The newest accepted tick.
        latest: Tick,
    },
    /// The peer charged more intents in one server-tick window than the
    /// session designed for.
    RateExceeded {
        /// The designed cap.
        limit: u32,
        /// The window it was reached in.
        window: u64,
    },
    /// The rate budget refused to track another peer rather than grow.
    TooManyPeers {
        /// The cap it refused to cross.
        max: usize,
    },
    /// The match phase does not allow this ask.
    WrongPhase {
        /// Where the match is.
        phase: Phase,
        /// The phases that would allow it.
        allowed: &'static str,
    },
}

impl IntentRefusal {
    /// The stable label used in reports.
    pub const fn label(&self) -> &'static str {
        match self {
            Self::ClientAuthoredTruth { .. } => "client_authored_truth",
            Self::NotOwned(_) => "invalid_ownership",
            Self::Loadout(_) => "loadout_rejected",
            Self::TickOutsideWindow { .. } => "tick_outside_window",
            Self::RateExceeded { .. } => "impossible_rate",
            Self::TooManyPeers { .. } => "resource_exhaustion",
            Self::WrongPhase { .. } => "wrong_phase",
        }
    }

    /// The threat class this refusal belongs to, when it is one. A rejected
    /// loadout or a wrong phase is an ordinary refusal — the client asked for
    /// something the rules do not allow — and names no threat.
    #[must_use]
    pub const fn threat(&self) -> Option<ThreatCase> {
        match self {
            Self::ClientAuthoredTruth { .. } => Some(ThreatCase::ClientAuthoredTruth),
            Self::NotOwned(_) => Some(ThreatCase::InvalidOwnership),
            Self::RateExceeded { .. } => Some(ThreatCase::ImpossibleRate),
            Self::TooManyPeers { .. } => Some(ThreatCase::ResourceExhaustion),
            Self::Loadout(_) | Self::TickOutsideWindow { .. } | Self::WrongPhase { .. } => None,
        }
    }

    /// What the session does about this refusal.
    ///
    /// Forged truth, foreign ownership and a blown rate/resource budget are
    /// abuse and disconnect the peer with this label as the bounded reason; a
    /// rejected loadout, a wrong phase or an out-of-window tick change nothing
    /// and the connection continues.
    ///
    /// [`Self::ClientAuthoredTruth`] is deliberately *not*
    /// [`ThreatCase::ClientAuthoredTruth`]'s disposition: at the wire layer
    /// (F58-A) that threat is `Structural`, because `ClientPayload` cannot
    /// express it. This is the independent later check — if a request the wire
    /// cannot carry reaches the validator anyway, the peer that sent it is not
    /// an honest client with a lossy link, and is cut off.
    #[must_use]
    pub const fn disposition(&self) -> ThreatDisposition {
        match self {
            Self::ClientAuthoredTruth { .. }
            | Self::NotOwned(_)
            | Self::RateExceeded { .. }
            | Self::TooManyPeers { .. } => ThreatDisposition::Disconnect,
            Self::Loadout(_) | Self::TickOutsideWindow { .. } | Self::WrongPhase { .. } => {
                ThreatDisposition::Absorb
            }
        }
    }
}

impl fmt::Display for IntentRefusal {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::ClientAuthoredTruth { domain } => write!(
                f,
                "a client may never author {domain:?}; the server owns that domain"
            ),
            Self::NotOwned(violation) => write!(f, "{violation}"),
            Self::Loadout(problem) => write!(f, "loadout refused: {problem:?}"),
            Self::TickOutsideWindow {
                tick,
                earliest,
                latest,
            } => write!(
                f,
                "input tick {} is outside the acceptance window {}..={}",
                tick.0, earliest.0, latest.0
            ),
            Self::RateExceeded { limit, window } => write!(
                f,
                "more than {limit} intents charged in server-tick window {window}"
            ),
            Self::TooManyPeers { max } => {
                write!(f, "the rate budget may track at most {max} peers")
            }
            Self::WrongPhase { phase, allowed } => {
                write!(f, "the match is {phase:?}, which allows {allowed}")
            }
        }
    }
}

impl std::error::Error for IntentRefusal {}

/// The outcome of judging one [`ClientIntent`]: the ask itself plus what the
/// server decided about it.
#[derive(Clone, Debug, PartialEq)]
pub enum IntentVerdict {
    /// Every check passed: this ask may be applied.
    Accepted(ClientIntent),
    /// Refused with a bounded reason that says what to do about the peer.
    Refused {
        /// The ask that was refused.
        intent: ClientIntent,
        /// Why.
        reason: IntentRefusal,
    },
}

impl IntentVerdict {
    /// Whether the ask was accepted.
    #[must_use]
    pub const fn accepted(&self) -> bool {
        matches!(self, Self::Accepted(_))
    }

    /// The refusal, when there was one.
    #[must_use]
    pub const fn refusal(&self) -> Option<&IntentRefusal> {
        match self {
            Self::Refused { reason, .. } => Some(reason),
            Self::Accepted(_) => None,
        }
    }

    /// The ask this verdict is about.
    #[must_use]
    pub const fn intent(&self) -> &ClientIntent {
        match self {
            Self::Accepted(intent) => intent,
            Self::Refused { intent, .. } => intent,
        }
    }
}

/// The host's intent validator: the per-peer rate budget plus the checks that
/// judge one ask against the server's actors, loadouts, tick window and match
/// state.
///
/// Every check runs in a fixed order and a refusal leaves all state alone:
///
/// 1. **server-owned truth** — a `Claim*` ask is refused before anything else
///    runs, and is not even charged to the budget;
/// 2. **rate** — the ask is charged against [`RateBudget`] in the *server's*
///    tick window, so a flooding peer is refused and disconnected;
/// 3. **server-owned actors** — an ask naming an aircraft must name one this
///    peer owns ([`ActorOwnership::authorize`]);
/// 4. **match state** — input needs a running match, a loadout change needs a
///    phase where loadouts are still open;
/// 5. **tick window** — input must sit inside
///    `[server_tick - MAX_INPUT_TICKS_BEHIND, server_tick + MAX_INPUT_TICKS_AHEAD]`;
/// 6. **loadout** — the asked-for loadout must be well formed and free of the
///    host's banned components.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct IntentValidator {
    budget: RateBudget,
}

impl IntentValidator {
    /// A validator enforcing `limits`.
    #[must_use]
    pub fn new(limits: RateLimits) -> Self {
        Self {
            budget: RateBudget::new(limits),
        }
    }

    /// A validator enforcing [`RateLimits::DESIGNED`].
    #[must_use]
    pub fn designed() -> Self {
        Self::new(RateLimits::DESIGNED)
    }

    /// The rate budget this validator charges.
    #[must_use]
    pub fn budget(&self) -> &RateBudget {
        &self.budget
    }

    /// Clears the budget, as a fresh session epoch does.
    pub fn reopen(&mut self) {
        self.budget.reset();
    }

    /// Judges one ask from `peer` against `ownership` and `stage`.
    ///
    /// # Errors
    ///
    /// The [`IntentRefusal`] naming the check that refused it. This function
    /// never panics and never applies anything itself.
    pub fn validate(
        &mut self,
        peer: PeerId,
        intent: &ClientIntent,
        ownership: &ActorOwnership,
        stage: MatchStage<'_>,
    ) -> Result<(), IntentRefusal> {
        // 1. A client never authors server-owned truth.
        if intent.asserts_truth() {
            return Err(IntentRefusal::ClientAuthoredTruth {
                domain: intent.domain(),
            });
        }
        // 2. Rate first, so every honest-looking ask still spends budget.
        if let Err(violation) = self.budget.charge(peer, stage.server_tick, 1) {
            return Err(match violation {
                RateViolation::Exceeded { limit, window } => {
                    IntentRefusal::RateExceeded { limit, window }
                }
                RateViolation::TooManyPeers { max } => IntentRefusal::TooManyPeers { max },
            });
        }
        // 3. Server-owned actors.
        if let Some(actor) = intent.actor() {
            ownership
                .authorize(peer, actor)
                .map_err(IntentRefusal::NotOwned)?;
        }
        // 4. Match state.
        match intent {
            ClientIntent::Input { .. } if stage.phase != Phase::InMatch => {
                return Err(IntentRefusal::WrongPhase {
                    phase: stage.phase,
                    allowed: "an InMatch session",
                });
            }
            ClientIntent::EquipLoadout { .. }
                if !matches!(stage.phase, Phase::Gathering | Phase::InMatch) =>
            {
                return Err(IntentRefusal::WrongPhase {
                    phase: stage.phase,
                    allowed: "a Gathering or InMatch session (a launch locks loadouts)",
                });
            }
            _ => {}
        }
        // 5. Tick window.
        if let ClientIntent::Input { tick, .. } = intent {
            let earliest = stage.server_tick.0.saturating_sub(MAX_INPUT_TICKS_BEHIND);
            let latest = stage.server_tick.0.saturating_add(MAX_INPUT_TICKS_AHEAD);
            if tick.0 < earliest || tick.0 > latest {
                return Err(IntentRefusal::TickOutsideWindow {
                    tick: *tick,
                    earliest: Tick(earliest),
                    latest: Tick(latest),
                });
            }
        }
        // 6. Loadout rules.
        if let ClientIntent::EquipLoadout { loadout } = intent {
            check_loadout_shape_and_bans(loadout, stage.banned_components)
                .map_err(IntentRefusal::Loadout)?;
        }
        Ok(())
    }
}

/// The intent-layer loadout judgement: shape, then the host's bans.
///
/// The shared F44 blueprint/budget judge ([`crate::lobby::LoadoutValidator`])
/// stays where it already runs — in the lobby's own readiness path (F55-B),
/// which `cs_net` cannot re-run from here without a wired validator. What this
/// layer adds is that an in-session ask cannot sidestep the host's *bans* or
/// hand in a malformed list.
///
/// # Errors
///
/// [`LoadoutProblem::Malformed`] for a duplicate, oversized or non-component
/// list, [`LoadoutProblem::Banned`] for one the host forbids.
fn check_loadout_shape_and_bans(
    loadout: &Loadout,
    banned: &BTreeSet<ContentId>,
) -> Result<(), LoadoutProblem> {
    let unique: BTreeSet<&ContentId> = loadout.components.iter().collect();
    if loadout.components.len() > MAX_LOADOUT_COMPONENTS
        || unique.len() != loadout.components.len()
        || loadout
            .components
            .iter()
            .any(|component| !is_component_kind(component.kind()))
    {
        return Err(LoadoutProblem::Malformed);
    }
    if let Some(component) = banned
        .iter()
        .find(|banned| loadout.blueprint == **banned || loadout.components.contains(banned))
    {
        return Err(LoadoutProblem::Banned {
            component: component.clone(),
        });
    }
    Ok(())
}
