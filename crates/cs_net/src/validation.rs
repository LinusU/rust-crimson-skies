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
//! Rate and resource caps (F58-B) and the disconnect/recovery flow (F58-C) are
//! deliberately absent: F58-A declares the threat and defines the typed input
//! and output, and nothing more.
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

use std::collections::BTreeMap;
use std::fmt;

use cs_types::Tick;
use cs_types::input::{Action, FlightCommand, InputFrame};
use cs_types::net::{ActorId, PeerId, SessionId};

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
