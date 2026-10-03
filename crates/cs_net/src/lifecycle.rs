//! The wired server and client session lifecycle (F54-C).
//!
//! Spec: `specs/F54-modern-multiplayer-transport-and-authority-protocol.md`,
//! stage `### F54-C` ("Wire the implemented path into its actual producer and
//! consumer; include teardown/retry and error propagation"). Contract:
//! `docs/contracts/UI-NETWORK.md`.
//!
//! # Why this module exists
//!
//! F54-A defined the vocabulary, F54-B built the pinned transport and proved
//! the session gate deduplicates replayed input. Neither is a *session*: there
//! was no owner for the launch/finish/teardown transitions, no producer turning
//! local input into wire packets, no consumer draining admitted input into the
//! simulation, and no retry for a lost packet. Every one of those gaps was
//! recorded as follow-up in `docs/findings/2026-10-03-f54-b-pinned-transport-
//! and-handshake.md`. This module closes them.
//!
//! * [`ServerSession`] is the host's session owner. It wraps [`HostTransport`],
//!   owns the phase machine ([`ServerPhase`]), drains admitted peer input into a
//!   **bounded** queue ([`drain_work`](ServerSession::drain_work)), publishes
//!   the reliable lifecycle events and snapshots the simulation produces, and
//!   hangs up on peers the declared threat model says to cut off.
//! * [`ClientSession`] is the client's session owner. It wraps
//!   [`ClientTransport`], owns [`ClientPhase`], **produces** wire input from
//!   locally sampled ticks ([`submit_sample`](ClientSession::submit_sample)),
//!   deduplicates reliable events by [`EventId`], keeps only the newest
//!   snapshot, tracks input acknowledgment, and retransmits a lost input packet
//!   verbatim.
//!
//! # What is bounded, and why
//!
//! Every queue in this module has a cap from [`crate::bounds`], because a
//! network peer is not a caller:
//!
//! * the host's work queue stops at [`MAX_WORK_PER_PUMP`]; the surplus is
//!   refused and the offending peer is disconnected (`ResourceExhaustion`);
//! * the client's dedup seen-set stops at [`MAX_SEEN_EVENTS`], evicting the
//!   *oldest* id so a long session cannot grow it without limit;
//! * the client's retransmit window stops at [`MAX_UNACKED_PACKETS`];
//! * the client's pending input stops at
//!   [`MAX_INPUT_FRAMES_PER_PACKET`], because a packet that carries more is
//!   unencodable anyway.
//!
//! # Error propagation
//!
//! Nothing is swallowed. Every refusal is a named value:
//! [`ServerFault`] / [`ClientFault`] for the actions a caller asked for that
//! could not be performed, [`ServerNotice::Dropped`] /
//! [`ClientNotice::Dropped`] for traffic refused on the wire, and
//! [`TransportFault`](ServerNotice::TransportFault) for a connection-layer
//! error. Teardown is explicit on both sides ([`ServerSession::close`],
//! [`ClientSession::leave`]) and repeatable ([`ServerSession::reopen`] mints a
//! fresh epoch for a retry, which is what makes every prior packet stale).
//!
//! Everything here is newly authored engine design on the F54-B transport. No
//! claim is made about the original game's networking, and no original
//! multiplayer content has been loaded.

use std::collections::{BTreeSet, VecDeque};
use std::fmt;
use std::net::SocketAddr;
use std::time::Duration;

use renet2::ClientId;

use cs_types::Tick;
use cs_types::content::ContentId;
use cs_types::input::{Action, AxisValue, AxisValueError, FlightCommand, InputFrame};
use cs_types::net::{ActorId, EventId, PeerId, SessionId};

use crate::bounds::{
    MAX_INPUT_FRAMES_PER_PACKET, MAX_SEEN_EVENTS, MAX_UNACKED_PACKETS, MAX_WORK_PER_PUMP,
};
use crate::codec::encode_client_message;
use crate::compat::{HandshakeReject, SessionGrant, SessionParameters};
use crate::message::{
    ClientMessage, ClientPayload, DisconnectReason, EventBody, FinishReason, InputBatch,
    MessageHeader, ReliableEvent, ServerMessage, ServerPayload, SnapshotFrame, WireError,
};
use crate::snapshot::{Snapshot, SnapshotError};
use crate::transport::{
    AdmittedInput, CHANNEL_SEQUENCED, ClientEvent, ClientTransport, DropReason, HostEvent,
    HostTransport, SendOutcome, TransportError,
};
use crate::validation::{Admission, FireRequest, SessionGate, ThreatCase, ThreatDisposition};

/// The `EventId::producer` the host's own lifecycle events carry. The host is
/// producer 0 in the session's event stream; gameplay systems own other
/// serials (F57/F56 publish through [`ServerSession::announce`]).
pub const HOST_EVENT_PRODUCER: u32 = 0;

/// How long a client waits, with no newer input to send and no acknowledgment
/// arriving, before retransmitting its oldest unacknowledged packet verbatim.
///
/// This is a retransmission *window*, not a rate limiter: the pacing and
/// per-tick request caps are F58-B's declared `ImpossibleRate` work, and this
/// module deliberately does not invent numbers for them.
pub const INPUT_RETRY_INTERVAL: Duration = Duration::from_millis(250);

// ------------------------------------------------------------------ host ----

/// The host session's phase. One epoch runs exactly one
/// `Gathering -> Live -> Finished` chain, and `Closed` is terminal.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ServerPhase {
    /// The session is open and admitting peers; no simulation is running.
    Gathering,
    /// The match launched; the simulation's first tick is `start_tick`.
    Live {
        /// The tick the session's simulation starts on.
        start_tick: Tick,
    },
    /// The match ended.
    Finished {
        /// Why it ended.
        reason: FinishReason,
    },
    /// Every peer is hung up and the epoch is spent. A retry is
    /// [`ServerSession::reopen`], which mints a *new* epoch.
    Closed,
}

impl ServerPhase {
    /// The stable label used in notices and reports.
    pub const fn label(self) -> &'static str {
        match self {
            Self::Gathering => "gathering",
            Self::Live { .. } => "live",
            Self::Finished { .. } => "finished",
            Self::Closed => "closed",
        }
    }

    /// Whether this phase still admits peers and session traffic.
    #[must_use]
    pub const fn open(self) -> bool {
        matches!(self, Self::Gathering | Self::Live { .. })
    }
}

impl fmt::Display for ServerPhase {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.label())
    }
}

/// One admitted client input packet, as the authoritative simulation consumes
/// it.
///
/// This is the host-side handoff of F54-C: nothing here exists without an
/// [`crate::validation::Admission::Accepted`] verdict from the session gate, so
/// a replayed or stale packet produces no [`Self::fires`] and no
/// [`Self::frames`].
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PeerInput {
    /// The peer that sent it.
    pub peer: PeerId,
    /// The admitted packet sequence.
    pub sequence: u32,
    /// The bounded tick-stamped frames, oldest first.
    pub frames: InputBatch,
    /// The fire requests weapon acceptance consumes, for the aircraft the
    /// server bound to `peer`. Empty when the peer has no aircraft yet.
    pub fires: Vec<FireRequest>,
}

impl PeerInput {
    fn from_admitted(peer: PeerId, admitted: AdmittedInput, fires: Vec<FireRequest>) -> Self {
        Self {
            peer,
            sequence: admitted.sequence,
            frames: admitted.batch,
            fires,
        }
    }
}

/// Why a host action could not be performed. Every variant names the action
/// and the reason, so a caller never has to infer *why* a session did not
/// move.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ServerFault {
    /// The action is not legal in the current phase.
    WrongPhase {
        /// What the caller asked for.
        action: &'static str,
        /// The phase the session is in.
        phase: ServerPhase,
    },
    /// The host's work queue was full, so the packet was refused and the peer
    /// disconnected rather than allowed to grow it.
    WorkQueueFull {
        /// The peer whose packet did not fit.
        peer: PeerId,
        /// The queue cap.
        limit: usize,
    },
    /// The transport could not be built or could not send.
    Transport(String),
    /// A record the host wanted to publish could not be encoded.
    Encode(String),
}

impl fmt::Display for ServerFault {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::WrongPhase { action, phase } => {
                write!(f, "cannot {action} while the session is {phase}")
            }
            Self::WorkQueueFull { peer, limit } => {
                write!(f, "{peer} overflowed the work queue of {limit} packets")
            }
            Self::Transport(reason) => write!(f, "transport error: {reason}"),
            Self::Encode(reason) => write!(f, "cannot encode for the wire: {reason}"),
        }
    }
}

impl std::error::Error for ServerFault {}

/// What one host pump observed, in receive order.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ServerNotice {
    /// The session changed phase.
    Phase(ServerPhase),
    /// A client completed the handshake and is now a member.
    PeerJoined {
        /// The peer the host allocated.
        peer: PeerId,
    },
    /// A member left, by `Leave` or by losing its connection.
    PeerLeft {
        /// The peer that is gone.
        peer: PeerId,
    },
    /// A client was turned away and has been told why.
    PeerRefused {
        /// The named reason.
        reason: HandshakeReject,
    },
    /// One admitted input packet, in the order it was admitted.
    Admitted(PeerInput),
    /// A buffer or a gate refusal produced nothing else. `peer` is `None` when
    /// the sender never held a peer id.
    Dropped {
        /// The peer the traffic came from, when it had one.
        peer: Option<PeerId>,
        /// Why the traffic was refused.
        reason: DropReason,
    },
    /// A gate refusal whose declared disposition is
    /// [`ThreatDisposition::Disconnect`]; the peer was hung up.
    CutOff {
        /// The peer that was hung up, when it held one.
        peer: Option<PeerId>,
        /// The threat class the refusal belongs to.
        threat: ThreatCase,
    },
    /// A connection with no peer id was hung up: a refused client, or one
    /// whose handshake answer could not be sent.
    HungUp {
        /// The peer id it held, when it held one.
        peer: Option<PeerId>,
        /// Why it was hung up.
        because: &'static str,
    },
    /// The connection layer reported an error. The session keeps running; the
    /// text is kept because `NetcodeTransportError` is not `Clone`.
    TransportFault {
        /// The reported error.
        reason: String,
    },
}

impl fmt::Display for ServerNotice {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Phase(phase) => write!(f, "session is now {phase}"),
            Self::PeerJoined { peer } => write!(f, "{peer} joined"),
            Self::PeerLeft { peer } => write!(f, "{peer} left"),
            Self::PeerRefused { reason } => write!(f, "a client was refused: {reason}"),
            Self::Admitted(input) => {
                write!(
                    f,
                    "admitted input {} from {} (sequence {}, {} fire request(s))",
                    input.frames.frames.len(),
                    input.peer,
                    input.sequence,
                    input.fires.len()
                )
            }
            Self::Dropped { peer, reason } => match peer {
                Some(peer) => write!(f, "dropped traffic from {peer}: {reason}"),
                None => write!(f, "dropped traffic from a client with no peer: {reason:?}"),
            },
            Self::CutOff { peer, threat } => match peer {
                Some(peer) => write!(f, "hung up on {peer}: {}", threat.label()),
                None => write!(f, "hung up on a client with no peer: {}", threat.label()),
            },
            Self::HungUp { peer, because } => match peer {
                Some(peer) => write!(f, "hung up on {peer} ({because})"),
                None => write!(f, "hung up on a client with no peer ({because})"),
            },
            Self::TransportFault { reason } => write!(f, "transport fault: {reason}"),
        }
    }
}

/// The host's session owner for one epoch.
///
/// It owns the [`HostTransport`] and therefore the session gate, so the replay
/// and ownership rules cannot be bypassed by a caller: admitted input is
/// already gated by the time a [`PeerInput`] exists.
pub struct ServerSession {
    transport: HostTransport,
    phase: ServerPhase,
    members: BTreeSet<PeerId>,
    work: VecDeque<PeerInput>,
    hung_up: Vec<ClientId>,
    next_event: u32,
    overflowed: u32,
}

impl ServerSession {
    /// Binds `addr` and opens the gathering session `session` under `params`.
    ///
    /// # Errors
    ///
    /// [`TransportError`] when the socket cannot bind; see
    /// [`HostTransport::bind`].
    pub fn bind(
        session: SessionId,
        params: SessionParameters,
        addr: SocketAddr,
        now: Duration,
    ) -> Result<Self, TransportError> {
        let transport = HostTransport::bind(session, params, addr, now)?;
        Ok(Self {
            transport,
            phase: ServerPhase::Gathering,
            members: BTreeSet::new(),
            work: VecDeque::new(),
            hung_up: Vec::new(),
            next_event: 0,
            overflowed: 0,
        })
    }

    /// The live session epoch.
    #[must_use]
    pub const fn session(&self) -> SessionId {
        self.transport.session()
    }

    /// The session's current phase.
    #[must_use]
    pub const fn phase(&self) -> ServerPhase {
        self.phase
    }

    /// The session gate (the ownership table an app binds aircraft through).
    #[must_use]
    pub fn gate(&self) -> &SessionGate {
        self.transport.gate()
    }

    /// The session gate, mutably, for aircraft binding.
    pub fn gate_mut(&mut self) -> &mut SessionGate {
        self.transport.gate_mut()
    }

    /// The address the host is reachable on.
    ///
    /// # Errors
    ///
    /// Propagates the transport's address lookup failure.
    pub fn local_addr(&self) -> std::io::Result<SocketAddr> {
        self.transport.local_addr()
    }

    /// The current members, in allocation order.
    pub fn members(&self) -> impl Iterator<Item = PeerId> + '_ {
        self.members.iter().copied()
    }

    /// How many peer input packets are waiting for the simulation.
    #[must_use]
    pub fn queued(&self) -> usize {
        self.work.len()
    }

    /// How many packets the bounded work queue has refused over this epoch.
    #[must_use]
    pub const fn overflowed(&self) -> u32 {
        self.overflowed
    }

    /// Takes the admitted input the simulation should apply this tick, oldest
    /// first, leaving the queue empty.
    ///
    /// This is the consumer entry point. It is bounded by
    /// [`MAX_WORK_PER_PUMP`] because [`Self::pump`] refuses the surplus.
    pub fn drain_work(&mut self) -> Vec<PeerInput> {
        self.work.drain(..).collect()
    }

    /// The phase this action requires, or the fault that explains the refusal.
    fn require(&self, action: &'static str, expected: ServerPhase) -> Result<(), ServerFault> {
        if self.phase == expected {
            Ok(())
        } else {
            Err(ServerFault::WrongPhase {
                action,
                phase: self.phase,
            })
        }
    }

    /// The phase this action requires, matching on a shape rather than a
    /// value (`Live { .. }` carries the start tick).
    fn require_shape(
        &self,
        action: &'static str,
        matches: fn(ServerPhase) -> bool,
    ) -> Result<(), ServerFault> {
        if matches(self.phase) {
            Ok(())
        } else {
            Err(ServerFault::WrongPhase {
                action,
                phase: self.phase,
            })
        }
    }

    /// Advances the connection layers and drains everything that arrived into
    /// session decisions.
    ///
    /// `elapsed` is the host tick's duration. The returned notices are ordered:
    /// membership changes first, then admitted input, then refusals.
    pub fn pump(&mut self, elapsed: Duration) -> Vec<ServerNotice> {
        let mut notices = Vec::new();

        // Hang up on connections the *previous* pump condemned. Deferring by one
        // round lets renet flush the reliable handshake answer or the last
        // `Disconnect` before the transport connection dies.
        let condemned = std::mem::take(&mut self.hung_up);
        for client in condemned {
            self.hang_up(client, "condemned last round", &mut notices);
        }

        let events = self.transport.update(elapsed);
        for event in events {
            match event {
                HostEvent::PeerJoined { peer } => {
                    self.members.insert(peer);
                    notices.push(ServerNotice::PeerJoined { peer });
                }
                HostEvent::PeerRejected { client, reason } => {
                    notices.push(ServerNotice::PeerRefused { reason });
                    self.hung_up.push(client);
                }
                HostEvent::ReplyFailed { client, reason } => {
                    notices.push(ServerNotice::TransportFault { reason });
                    self.hung_up.push(client);
                }
                HostEvent::PeerDeparted { peer } => {
                    self.members.remove(&peer);
                    notices.push(ServerNotice::PeerLeft { peer });
                }
                HostEvent::PeerPacket {
                    peer,
                    admission,
                    input,
                    fires,
                } => match (&admission, input) {
                    (Admission::Accepted { .. }, Some(admitted)) => {
                        self.enqueue(
                            PeerInput::from_admitted(peer, admitted, fires),
                            &mut notices,
                        );
                    }
                    _ => {
                        if let Some(violation) = admission.violation() {
                            // Every refusal is reported, absorbed or not: a
                            // replay that produced no work must still be
                            // visible to the caller, or "absorbed" and
                            // "silently lost" look identical from outside.
                            notices.push(ServerNotice::Dropped {
                                peer: Some(peer),
                                reason: DropReason::Refused(violation.clone()),
                            });
                            if violation.disposition() == ThreatDisposition::Disconnect {
                                notices.push(ServerNotice::CutOff {
                                    peer: Some(peer),
                                    threat: violation.threat(),
                                });
                                self.members.remove(&peer);
                                self.transport.disconnect_peer(peer);
                            }
                        }
                    }
                },
                HostEvent::PacketDropped { client, reason } => {
                    let peer = self.transport.peer_of(client);
                    notices.push(ServerNotice::Dropped {
                        peer,
                        reason: reason.clone(),
                    });
                    // The declared dispositions decide: an oversized buffer or
                    // traffic from a client that holds no peer id is abuse to be
                    // cut off; an extra hello or any other malformed buffer is
                    // absorbed, exactly as `ThreatCase::disposition` declares.
                    let abuse = match &reason {
                        DropReason::Decode(reason) if reason.is_oversized() => {
                            Some(ThreatCase::OversizedMessage)
                        }
                        DropReason::NoPeer => Some(ThreatCase::UnauthenticatedPeer),
                        DropReason::Decode(_)
                        | DropReason::ExtraHello
                        | DropReason::QueueOverflow { .. }
                        | DropReason::Refused(_) => None,
                    };
                    if let Some(threat) = abuse {
                        notices.push(ServerNotice::CutOff { peer, threat });
                        self.hung_up.push(client);
                    }
                }
                HostEvent::TransportFault { reason } => {
                    notices.push(ServerNotice::TransportFault { reason });
                }
            }
        }
        notices
    }

    /// Queues one admitted packet, or refuses it and disconnects the peer when
    /// the queue is at [`MAX_WORK_PER_PUMP`].
    fn enqueue(&mut self, input: PeerInput, notices: &mut Vec<ServerNotice>) {
        if self.work.len() >= MAX_WORK_PER_PUMP {
            self.overflowed = self.overflowed.saturating_add(1);
            notices.push(ServerNotice::Dropped {
                peer: Some(input.peer),
                reason: DropReason::QueueOverflow {
                    limit: MAX_WORK_PER_PUMP,
                },
            });
            notices.push(ServerNotice::CutOff {
                peer: Some(input.peer),
                threat: ThreatCase::ResourceExhaustion,
            });
            self.members.remove(&input.peer);
            self.transport.disconnect_peer(input.peer);
            return;
        }
        notices.push(ServerNotice::Admitted(input.clone()));
        self.work.push_back(input);
    }

    /// Hangs up on one transport connection and records why.
    fn hang_up(
        &mut self,
        client: ClientId,
        because: &'static str,
        notices: &mut Vec<ServerNotice>,
    ) {
        let peer = self.transport.peer_of(client);
        if let Some(peer) = peer {
            self.members.remove(&peer);
        }
        self.transport.disconnect_client(client);
        notices.push(ServerNotice::HungUp { peer, because });
    }

    /// Announces the server-allocated spawn of an actor.
    ///
    /// # Errors
    ///
    /// [`ServerFault::Encode`] when the record cannot be encoded.
    pub fn announce_spawn(
        &mut self,
        tick: Tick,
        actor: ActorId,
        blueprint: ContentId,
        owner: Option<PeerId>,
    ) -> Result<(), ServerFault> {
        self.announce(
            tick,
            EventBody::ActorSpawned {
                actor,
                blueprint,
                owner,
            },
        )
    }

    /// Announces the removal of an actor.
    ///
    /// # Errors
    ///
    /// [`ServerFault::Encode`] when the record cannot be encoded.
    pub fn announce_removal(&mut self, tick: Tick, actor: ActorId) -> Result<(), ServerFault> {
        self.announce(tick, EventBody::ActorRemoved { actor })
    }

    /// Publishes one reliable, idempotent lifecycle event to every member.
    ///
    /// This is the publisher the domain stages (F55 lobby outcomes, F56 mode
    /// rules, F57 gameplay events) send through, so nothing bypasses the
    /// [`EventId`] dedup discipline the contract requires.
    ///
    /// # Errors
    ///
    /// [`ServerFault::Encode`] when the event cannot be encoded, or
    /// [`ServerFault::Transport`] when the transport refuses the send.
    pub fn announce(&mut self, tick: Tick, body: EventBody) -> Result<(), ServerFault> {
        let id = EventId {
            session: self.session(),
            tick,
            producer: HOST_EVENT_PRODUCER,
            sequence: self.next_event,
        };
        self.next_event = self.next_event.saturating_add(1);
        self.broadcast(ServerPayload::Event(ReliableEvent { id, body }))
    }

    /// Publishes the simulation's snapshot for `tick` to every member.
    ///
    /// The snapshot is encoded and size-checked here, so a payload the bounded
    /// schema refuses never reaches the wire and no client has to decode it.
    ///
    /// # Errors
    ///
    /// [`ServerFault::Encode`] when the snapshot cannot be encoded or exceeds
    /// the packet caps.
    pub fn publish_snapshot(&mut self, tick: Tick, snapshot: &Snapshot) -> Result<(), ServerFault> {
        let payload = snapshot
            .encode(self.session())
            .map_err(|reason| ServerFault::Encode(reason.to_string()))?;
        self.broadcast(ServerPayload::Snapshot(SnapshotFrame { tick, payload }))
    }

    /// Acknowledges `peer`'s input up to and including `through`, which lets
    /// the client stop retransmitting those packets.
    ///
    /// # Errors
    ///
    /// [`ServerFault::WrongPhase`] when the epoch is spent, or
    /// [`ServerFault::Transport`] when the send fails.
    pub fn acknowledge_input(
        &mut self,
        peer: PeerId,
        through: u32,
    ) -> Result<SendOutcome, ServerFault> {
        if !self.phase.open() {
            return Err(ServerFault::WrongPhase {
                action: "acknowledge input",
                phase: self.phase,
            });
        }
        self.send(peer, ServerPayload::InputAck { through })
            .map_err(|reason| ServerFault::Transport(reason.to_string()))
    }

    /// Launches the match: the session moves from `Gathering` to `Live` and
    /// every member is told the start tick on the reliable channel.
    ///
    /// # Errors
    ///
    /// [`ServerFault::WrongPhase`] when the match already launched or the
    /// session is closed, [`ServerFault::Encode`] when the event cannot be
    /// encoded, [`ServerFault::Transport`] when the send fails.
    pub fn launch(&mut self, start_tick: Tick) -> Result<(), ServerFault> {
        self.require("launch", ServerPhase::Gathering)?;
        self.phase = ServerPhase::Live { start_tick };
        self.announce(start_tick, EventBody::Launched { start_tick })
    }

    /// Ends the match and tells every member why.
    ///
    /// # Errors
    ///
    /// As [`Self::launch`], with `Gathering`/`Live` replaced by `Live`.
    pub fn finish(&mut self, tick: Tick, reason: FinishReason) -> Result<(), ServerFault> {
        self.require_shape("finish", |phase| matches!(phase, ServerPhase::Live { .. }))?;
        self.announce(tick, EventBody::Finished { reason })?;
        self.phase = ServerPhase::Finished { reason };
        Ok(())
    }

    /// Tears the session down: every member is told `reason` on the reliable
    /// channel and then hung up, and the epoch is spent.
    ///
    /// Idempotent — a closed session reports zero hung up. This is the
    /// teardown half of F54-C; [`Self::reopen`] is the retry half.
    ///
    /// # Errors
    ///
    /// [`ServerFault::Transport`] when the goodbye cannot be encoded or sent.
    pub fn close(&mut self, reason: DisconnectReason) -> Result<usize, ServerFault> {
        if self.phase == ServerPhase::Closed {
            return Ok(0);
        }
        self.broadcast(ServerPayload::Disconnect { reason })?;
        let peers: Vec<PeerId> = self.members.iter().copied().collect();
        for peer in &peers {
            self.transport.disconnect_peer(*peer);
        }
        self.members.clear();
        self.work.clear();
        self.phase = ServerPhase::Closed;
        Ok(peers.len())
    }

    /// Mints a fresh epoch on the same socket for a retry.
    ///
    /// Every packet stamped with the previous epoch is stale by construction
    /// (contract: "Epoch mismatch rejects stale packets"), so a returning
    /// client cannot continue the old session even if it replays a sequence
    /// number from it. The socket is kept, so a retry does not depend on the
    /// same port still being free.
    pub fn reopen(&mut self, session: SessionId) -> Result<(), ServerFault> {
        self.close(DisconnectReason::SessionEnded)?;
        self.transport.reopen(session);
        self.next_event = 0;
        self.overflowed = 0;
        self.phase = ServerPhase::Gathering;
        Ok(())
    }

    /// Sends one payload to one peer, stamped by the transport.
    fn send(
        &mut self,
        peer: PeerId,
        payload: ServerPayload,
    ) -> Result<SendOutcome, TransportError> {
        self.transport.send(
            peer,
            ServerMessage {
                header: MessageHeader {
                    session: self.session(),
                    sequence: 0,
                },
                payload,
            },
        )
    }

    /// Sends one payload to every member, stamped once.
    fn broadcast(&mut self, payload: ServerPayload) -> Result<(), ServerFault> {
        self.transport
            .broadcast(ServerMessage {
                header: MessageHeader {
                    session: self.session(),
                    sequence: 0,
                },
                payload,
            })
            .map_err(|reason| ServerFault::Transport(reason.to_string()))
    }
}

impl fmt::Debug for ServerSession {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("ServerSession")
            .field("session", &self.session())
            .field("phase", &self.phase)
            .field("members", &self.members)
            .field("queued", &self.work.len())
            .field("overflowed", &self.overflowed)
            .finish_non_exhaustive()
    }
}

// ---------------------------------------------------------------- client ----

/// The client session's phase.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ClientPhase {
    /// The transport is built but the connection is not up.
    Connecting,
    /// The connection is up and the hello is on its way; no verdict yet.
    Joining,
    /// The host granted a session; session traffic is allowed, no launch yet.
    Joined,
    /// The host published `Launched`.
    Live {
        /// The tick the match starts on.
        start_tick: Tick,
    },
    /// The host published `Finished`.
    Finished {
        /// Why the match ended.
        reason: FinishReason,
    },
    /// Terminal: the session is over and the transport is hung up.
    Closed(ClientClosure),
}

impl ClientPhase {
    /// The stable label used in notices and reports.
    #[must_use]
    pub const fn label(&self) -> &'static str {
        match self {
            Self::Connecting => "connecting",
            Self::Joining => "joining",
            Self::Joined => "joined",
            Self::Live { .. } => "live",
            Self::Finished { .. } => "finished",
            Self::Closed(_) => "closed",
        }
    }

    /// Whether this phase still sends session traffic.
    #[must_use]
    pub const fn open(&self) -> bool {
        matches!(
            self,
            Self::Joined | Self::Live { .. } | Self::Finished { .. }
        )
    }

    /// The closure reason, when this phase is terminal.
    #[must_use]
    pub const fn closure(&self) -> Option<&ClientClosure> {
        match self {
            Self::Closed(reason) => Some(reason),
            _ => None,
        }
    }
}

impl fmt::Display for ClientPhase {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.label())
    }
}

/// Why a client session ended.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ClientClosure {
    /// The host refused the handshake. The reason is the one the client was
    /// told, so the UI can name it (spec F54 AC01).
    Refused(HandshakeReject),
    /// The host closed the session or dropped this peer.
    ServerClosed(DisconnectReason),
    /// This client left.
    LeftVoluntarily,
    /// The connection was lost. The text is the transport's own report.
    TransportLost(String),
}

impl fmt::Display for ClientClosure {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Refused(reason) => write!(f, "refused: {reason}"),
            Self::ServerClosed(reason) => write!(f, "host closed the session: {}", reason.label()),
            Self::LeftVoluntarily => f.write_str("left voluntarily"),
            Self::TransportLost(reason) => write!(f, "connection lost: {reason}"),
        }
    }
}

/// Why a client action could not be performed, or why inbound traffic was
/// refused.
#[derive(Clone, Debug, PartialEq)]
pub enum ClientFault {
    /// The session has no grant yet, so there is nothing to send on.
    NotInSession,
    /// The session is closed.
    Closed {
        /// Why it closed.
        closure: ClientClosure,
    },
    /// The locally sampled value could not be quantized: NaN, infinite,
    /// outside `[-1, 1]`, or an edge command rather than an axis.
    ///
    /// This is the door a non-finite local sample is refused at, before it can
    /// become a wire field. No wire field is a float, so refusing here is what
    /// keeps "finite numeric fields" true end to end.
    Axis(AxisValueError),
    /// The pending input queue is at [`MAX_INPUT_FRAMES_PER_PACKET`].
    InputQueueFull {
        /// The cap.
        max: usize,
    },
    /// A sampled tick is not newer than the last one queued, so it would break
    /// the batch's strict tick order and fail the wire validation.
    TickNotNewer {
        /// The refused tick.
        tick: Tick,
        /// The last tick already queued.
        previous: Tick,
    },
    /// The packet named a session epoch this client is not in, or violated the
    /// wire bounds.
    Wire(WireError),
    /// The snapshot payload failed the bounded schema.
    Snapshot(SnapshotError),
    /// The transport refused the send.
    Transport(String),
}

impl fmt::Display for ClientFault {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::NotInSession => {
                f.write_str("no session grant yet: the handshake must come first")
            }
            Self::Closed { closure } => write!(f, "the session is closed ({closure})"),
            Self::Axis(reason) => write!(f, "cannot quantize a local sample: {reason}"),
            Self::InputQueueFull { max } => {
                write!(f, "the pending input queue is full ({max} frames)")
            }
            Self::TickNotNewer { tick, previous } => {
                write!(
                    f,
                    "tick {} does not follow the queued tick {}",
                    tick.0, previous.0
                )
            }
            Self::Wire(reason) => write!(f, "refused an inbound packet: {reason}"),
            Self::Snapshot(reason) => write!(f, "refused a snapshot payload: {reason}"),
            Self::Transport(reason) => write!(f, "transport error: {reason}"),
        }
    }
}

impl std::error::Error for ClientFault {}

/// What one client pump observed, in receive order.
#[derive(Clone, Debug, PartialEq)]
pub enum ClientNotice {
    /// The session changed phase.
    Phase(ClientPhase),
    /// The host granted a session.
    Joined {
        /// The grant: the epoch and peer id every later packet is stamped with.
        grant: SessionGrant,
    },
    /// The host refused the handshake, with the named reason.
    Refused {
        /// Why.
        reason: HandshakeReject,
    },
    /// A reliable event was accepted for the first time. `body` is the fact;
    /// the client applies it to its own presentation state.
    Event {
        /// The event's deduplication id.
        id: EventId,
        /// What happened.
        body: EventBody,
    },
    /// A reliable event arrived whose id this client already applied. The
    /// reliable channel can replay after a retry; application idempotency is
    /// what stops it taking effect twice.
    Duplicate {
        /// The repeated id.
        id: EventId,
    },
    /// A motion snapshot was accepted as the newest so far.
    Snapshot {
        /// The tick it describes.
        tick: Tick,
    },
    /// A snapshot named a tick at or below the one already held, so it was
    /// dropped: snapshots are sequenced and droppable, and an older frame must
    /// never overwrite a newer one.
    StaleSnapshot {
        /// The refused tick.
        tick: Tick,
    },
    /// The host acknowledged the client's input through this sequence.
    Acked {
        /// The highest consumed client sequence.
        through: u32,
    },
    /// An acknowledgment did not advance past the one already held.
    StaleAck {
        /// The refused acknowledgment.
        through: u32,
        /// The acknowledgment already held.
        acknowledged: u32,
    },
    /// The oldest unacknowledged input packet was retransmitted verbatim. Its
    /// sequence is unchanged, so a host that already has it absorbs the replay
    /// (spec F54 AC02) and a host that lost it applies the retry.
    Retried {
        /// The retransmitted packet's sequence.
        sequence: u32,
    },
    /// A buffer or record was refused; it changed nothing.
    Dropped {
        /// Why.
        reason: ClientFault,
    },
    /// The connection went down.
    Disconnected {
        /// Why the session closed.
        closure: ClientClosure,
    },
}

impl fmt::Display for ClientNotice {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Phase(phase) => write!(f, "client session is now {phase}"),
            Self::Joined { grant } => {
                write!(f, "joined session {} as {}", grant.session, grant.peer)
            }
            Self::Refused { reason } => write!(f, "refused by the host: {reason}"),
            Self::Event { id, body } => write!(f, "event {id}: {body:?}"),
            Self::Duplicate { id } => write!(f, "event {id} arrived again and was ignored"),
            Self::Snapshot { tick } => write!(f, "accepted the snapshot for tick {}", tick.0),
            Self::StaleSnapshot { tick } => {
                write!(f, "dropped the stale snapshot for tick {}", tick.0)
            }
            Self::Acked { through } => write!(f, "input acknowledged through {through}"),
            Self::StaleAck {
                through,
                acknowledged,
            } => {
                write!(
                    f,
                    "acknowledgment {through} does not advance past {acknowledged}"
                )
            }
            Self::Retried { sequence } => write!(f, "retransmitted input {sequence}"),
            Self::Dropped { reason } => write!(f, "dropped traffic: {reason}"),
            Self::Disconnected { closure } => write!(f, "disconnected: {closure}"),
        }
    }
}

/// One sent-but-unacknowledged input packet, kept as the exact bytes that were
/// emitted so a retry is byte-identical.
#[derive(Clone, Debug, PartialEq, Eq)]
struct Unacked {
    sequence: u32,
    bytes: Vec<u8>,
}

/// The client session owner for one connection.
pub struct ClientSession {
    transport: ClientTransport,
    phase: ClientPhase,
    pending: Vec<InputFrame>,
    unacked: VecDeque<Unacked>,
    acked_through: Option<u32>,
    seen: BTreeSet<EventId>,
    latest: Option<(Tick, Snapshot)>,
    since_retry: Duration,
}

impl ClientSession {
    /// Connects to `server_addr` offering `hello`.
    ///
    /// # Errors
    ///
    /// [`TransportError`] when the socket cannot bind or the client transport
    /// cannot be built.
    pub fn connect(
        hello: crate::compat::ClientHello,
        server_addr: SocketAddr,
        client_id: u64,
        now: Duration,
    ) -> Result<Self, TransportError> {
        Ok(Self {
            transport: ClientTransport::connect(hello, server_addr, client_id, now)?,
            phase: ClientPhase::Connecting,
            pending: Vec::new(),
            unacked: VecDeque::new(),
            acked_through: None,
            seen: BTreeSet::new(),
            latest: None,
            since_retry: Duration::ZERO,
        })
    }

    /// The session's current phase.
    #[must_use]
    pub const fn phase(&self) -> &ClientPhase {
        &self.phase
    }

    /// The grant the host issued, once the handshake succeeded.
    #[must_use]
    pub const fn grant(&self) -> Option<SessionGrant> {
        self.transport.grant()
    }

    /// How many locally sampled frames are waiting for the next packet.
    #[must_use]
    pub fn pending(&self) -> usize {
        self.pending.len()
    }

    /// How many sent packets are still unacknowledged.
    #[must_use]
    pub fn unacked(&self) -> usize {
        self.unacked.len()
    }

    /// The highest input sequence the host acknowledged.
    #[must_use]
    pub const fn acked_through(&self) -> Option<u32> {
        self.acked_through
    }

    /// How many reliable-event ids this client remembers for deduplication.
    #[must_use]
    pub fn remembered_events(&self) -> usize {
        self.seen.len()
    }

    /// The newest accepted snapshot, which is what the interpolation buffer
    /// (F57-B) consumes.
    #[must_use]
    pub fn snapshot(&self) -> Option<&Snapshot> {
        self.latest.as_ref().map(|(_, snapshot)| snapshot)
    }

    /// The tick of the newest accepted snapshot.
    #[must_use]
    pub fn snapshot_tick(&self) -> Option<Tick> {
        self.latest.as_ref().map(|(tick, _)| *tick)
    }

    /// The underlying client transport, for the caller that needs the raw
    /// connection (address, disconnect).
    #[must_use]
    pub const fn transport(&self) -> &ClientTransport {
        &self.transport
    }

    /// Quantizes one locally sampled axis value for the wire.
    ///
    /// This is the client-side producer's numeric boundary: a NaN or infinite
    /// sample from a device, a driver or a dividing-by-zero binding is refused
    /// here and never becomes a packet.
    ///
    /// # Errors
    ///
    /// [`ClientFault::Axis`] when the command is not a continuous axis or the
    /// value is non-finite or out of the normalized range.
    pub fn sample_axis(command: FlightCommand, value: f32) -> Result<AxisValue, ClientFault> {
        AxisValue::from_unit(command, value).map_err(ClientFault::Axis)
    }

    /// Samples one continuous axis on `tick` and queues it for the next input
    /// packet.
    ///
    /// # Errors
    ///
    /// [`ClientFault::Axis`] for a non-finite or out-of-range sample,
    /// [`ClientFault::Closed`] when the session is over,
    /// [`ClientFault::TickNotNewer`] when `tick` does not follow the queued
    /// frames, and [`ClientFault::InputQueueFull`] at
    /// [`MAX_INPUT_FRAMES_PER_PACKET`].
    pub fn submit_sample(
        &mut self,
        command: FlightCommand,
        value: f32,
        tick: Tick,
    ) -> Result<(), ClientFault> {
        let axis = Self::sample_axis(command, value)?;
        self.push(tick, |frame| frame.set_axis(axis))
    }

    /// Records one one-shot flight edge on `tick` and queues it for the next
    /// input packet.
    ///
    /// Only a [`FlightCommand`] can be submitted, never a [`cs_types::input::UiAction`]:
    /// UI state is client-owned and must never cross the wire (contract), so
    /// the producer cannot express it.
    ///
    /// # Errors
    ///
    /// As [`Self::submit_sample`], minus [`ClientFault::Axis`].
    pub fn submit_edge(&mut self, command: FlightCommand, tick: Tick) -> Result<(), ClientFault> {
        self.push(tick, |frame| frame.push_edge(Action::Flight(command)))
    }

    /// Appends to the pending batch, extending the last frame when the tick
    /// matches it.
    fn push(&mut self, tick: Tick, add: impl FnOnce(&mut InputFrame)) -> Result<(), ClientFault> {
        if let ClientPhase::Closed(closure) = &self.phase {
            return Err(ClientFault::Closed {
                closure: closure.clone(),
            });
        }
        if let Some(previous) = self.pending.last().map(InputFrame::frame_tick)
            && tick < previous
        {
            return Err(ClientFault::TickNotNewer { tick, previous });
        }
        if self.pending.len() >= MAX_INPUT_FRAMES_PER_PACKET
            && self.pending.last().map(InputFrame::frame_tick) != Some(tick)
        {
            return Err(ClientFault::InputQueueFull {
                max: MAX_INPUT_FRAMES_PER_PACKET,
            });
        }
        match self.pending.last_mut() {
            Some(frame) if frame.frame_tick() == tick => add(frame),
            _ => {
                let mut frame = InputFrame::new(tick);
                add(&mut frame);
                self.pending.push(frame);
            }
        }
        Ok(())
    }

    /// Advances the connection layers, dispatches everything that arrived and
    /// sends what the local input queue has accumulated.
    pub fn pump(&mut self, elapsed: Duration) -> Vec<ClientNotice> {
        let mut notices = Vec::new();
        for event in self.transport.update(elapsed) {
            self.handle(event, &mut notices);
        }
        if self.phase.open() {
            self.flush(&mut notices);
            self.retry(elapsed, &mut notices);
        }
        notices
    }

    /// Turns one transport event into notices and phase changes.
    fn handle(&mut self, event: ClientEvent, notices: &mut Vec<ClientNotice>) {
        match event {
            ClientEvent::Connected => self.move_to(ClientPhase::Joining, notices),
            ClientEvent::Granted { grant } => {
                notices.push(ClientNotice::Joined { grant });
                self.move_to(ClientPhase::Joined, notices);
            }
            ClientEvent::Rejected { reason } => {
                notices.push(ClientNotice::Refused {
                    reason: reason.clone(),
                });
                self.move_to(ClientPhase::Closed(ClientClosure::Refused(reason)), notices);
            }
            ClientEvent::Server(message) => match self.accept(message) {
                Ok(applied) => notices.extend(applied),
                Err(reason) => notices.push(ClientNotice::Dropped { reason }),
            },
            ClientEvent::Disconnected { reason } => {
                if !matches!(self.phase, ClientPhase::Closed(_)) {
                    self.move_to(
                        ClientPhase::Closed(ClientClosure::TransportLost(format!("{reason:?}"))),
                        notices,
                    );
                    notices.push(ClientNotice::Disconnected {
                        closure: self
                            .phase
                            .closure()
                            .cloned()
                            .unwrap_or(ClientClosure::TransportLost(format!("{reason:?}"))),
                    });
                }
            }
            ClientEvent::PacketDropped { reason } => notices.push(ClientNotice::Dropped {
                reason: ClientFault::Transport(format!("undecodable server packet: {reason}")),
            }),
            ClientEvent::TransportFault { reason } => {
                notices.push(ClientNotice::Dropped {
                    reason: ClientFault::Transport(reason.clone()),
                });
                if !matches!(self.phase, ClientPhase::Closed(_)) {
                    self.move_to(
                        ClientPhase::Closed(ClientClosure::TransportLost(reason)),
                        notices,
                    );
                }
            }
        }
    }

    /// Applies one decoded server message.
    ///
    /// This is the client's consumer entry point and is exactly what
    /// [`Self::pump`] runs for every arrived packet: the epoch check first
    /// (a stale packet can change nothing), then deduplication for reliable
    /// events, monotonic selection for snapshots and the acknowledgment
    /// window for input.
    ///
    /// # Errors
    ///
    /// [`ClientFault::NotInSession`] before the handshake granted a session and
    /// [`ClientFault::Wire`] when the packet names another epoch.
    pub fn accept(&mut self, message: ServerMessage) -> Result<Vec<ClientNotice>, ClientFault> {
        let Some(grant) = self.transport.grant() else {
            return Err(ClientFault::NotInSession);
        };
        message
            .expect_session(grant.session)
            .map_err(ClientFault::Wire)?;
        // The envelope's own bounds run again here. The codec validates every
        // decoded packet, but `accept` is a public entry point, and an
        // oversized snapshot payload must not reach a consumer whoever built
        // the message.
        message.validate().map_err(ClientFault::Wire)?;
        let mut notices = Vec::new();
        match message.payload {
            ServerPayload::Event(event) => {
                if !self.remember(event.id) {
                    notices.push(ClientNotice::Duplicate { id: event.id });
                    return Ok(notices);
                }
                match &event.body {
                    EventBody::Launched { start_tick } => {
                        self.move_to(
                            ClientPhase::Live {
                                start_tick: *start_tick,
                            },
                            &mut notices,
                        );
                    }
                    EventBody::Finished { reason } => {
                        self.move_to(ClientPhase::Finished { reason: *reason }, &mut notices);
                    }
                    _ => {}
                }
                notices.push(ClientNotice::Event {
                    id: event.id,
                    body: event.body,
                });
            }
            ServerPayload::Snapshot(frame) => {
                if self
                    .latest
                    .as_ref()
                    .is_some_and(|(tick, _)| frame.tick <= *tick)
                {
                    notices.push(ClientNotice::StaleSnapshot { tick: frame.tick });
                    return Ok(notices);
                }
                // The payload is decoded and schema-checked here, so what the
                // interpolation buffer receives is a bounded, validated record
                // set and never raw bytes.
                let snapshot = Snapshot::decode(&frame.payload, grant.session)
                    .map_err(ClientFault::Snapshot)?;
                self.latest = Some((frame.tick, snapshot));
                notices.push(ClientNotice::Snapshot { tick: frame.tick });
            }
            ServerPayload::InputAck { through } => {
                if let Some(acknowledged) = self.acked_through
                    && through <= acknowledged
                {
                    notices.push(ClientNotice::StaleAck {
                        through,
                        acknowledged,
                    });
                    return Ok(notices);
                }
                self.acked_through = Some(through);
                while self
                    .unacked
                    .front()
                    .is_some_and(|sent| sent.sequence <= through)
                {
                    self.unacked.pop_front();
                }
                notices.push(ClientNotice::Acked { through });
            }
            ServerPayload::Disconnect { reason } => {
                self.move_to(
                    ClientPhase::Closed(ClientClosure::ServerClosed(reason)),
                    &mut notices,
                );
            }
        }
        Ok(notices)
    }

    /// Records an event id, evicting the oldest once the seen-set is full.
    fn remember(&mut self, id: EventId) -> bool {
        if !self.seen.insert(id) {
            return false;
        }
        while self.seen.len() > MAX_SEEN_EVENTS
            && let Some(oldest) = self.seen.iter().next().copied()
        {
            self.seen.remove(&oldest);
        }
        true
    }

    /// Records a phase change and its notice.
    fn move_to(&mut self, phase: ClientPhase, notices: &mut Vec<ClientNotice>) {
        if self.phase == phase {
            return;
        }
        self.phase = phase.clone();
        notices.push(ClientNotice::Phase(phase));
    }

    /// Sends the pending frames as one bounded input packet and remembers it
    /// for retransmission.
    fn flush(&mut self, notices: &mut Vec<ClientNotice>) {
        if self.pending.is_empty() {
            return;
        }
        let Some(grant) = self.transport.grant() else {
            notices.push(ClientNotice::Dropped {
                reason: ClientFault::NotInSession,
            });
            return;
        };
        let batch = InputBatch {
            frames: std::mem::take(&mut self.pending),
        };
        let mut message = ClientMessage {
            header: MessageHeader {
                session: grant.session,
                sequence: 0,
            },
            payload: ClientPayload::Input(batch),
        };
        let sequence = match self.transport.send(message.clone()) {
            Ok(sequence) => sequence,
            Err(reason) => {
                notices.push(ClientNotice::Dropped {
                    reason: ClientFault::Transport(reason.to_string()),
                });
                if let ClientPayload::Input(batch) = message.payload {
                    self.pending = batch.frames;
                }
                return;
            }
        };
        // Re-encode with the stamped sequence so the retained bytes are
        // byte-identical to what went out: a retransmission must not look like
        // a new packet.
        message.header.sequence = sequence;
        let bytes = match encode_client_message(&message) {
            Ok(bytes) => bytes,
            Err(reason) => {
                notices.push(ClientNotice::Dropped {
                    reason: ClientFault::Transport(format!("unencodable input: {reason}")),
                });
                return;
            }
        };
        if self.unacked.len() >= MAX_UNACKED_PACKETS {
            // Bounded: the oldest unacknowledged packet is forgotten rather
            // than letting a host that never acknowledges grow this window. The
            // sequence is not recycled, so the host's replay window still
            // refuses it if it ever arrives.
            self.unacked.pop_front();
        }
        self.unacked.push_back(Unacked { sequence, bytes });
    }

    /// Retransmits the oldest unacknowledged packet when there is nothing newer
    /// to send.
    fn retry(&mut self, elapsed: Duration, notices: &mut Vec<ClientNotice>) {
        self.since_retry = self.since_retry.saturating_add(elapsed);
        if !self.pending.is_empty() || self.since_retry < INPUT_RETRY_INTERVAL {
            return;
        }
        self.since_retry = Duration::ZERO;
        let Some(oldest) = self.unacked.front().cloned() else {
            return;
        };
        // The exact bytes, so the retry carries the same sequence and the host
        // absorbs it if it already has the packet (F54 AC02) while a host that
        // lost it applies the retry.
        self.transport
            .send_encoded(CHANNEL_SEQUENCED, &oldest.bytes);
        notices.push(ClientNotice::Retried {
            sequence: oldest.sequence,
        });
    }

    /// Leaves the session: tells the host on the reliable channel, then hangs
    /// up.
    ///
    /// # Errors
    ///
    /// [`ClientFault::NotInSession`] before the handshake, or
    /// [`ClientFault::Transport`] when the farewell cannot be sent.
    pub fn leave(&mut self) -> Result<(), ClientFault> {
        if let ClientPhase::Closed(closure) = &self.phase {
            return Err(ClientFault::Closed {
                closure: closure.clone(),
            });
        }
        let Some(grant) = self.transport.grant() else {
            return Err(ClientFault::NotInSession);
        };
        self.transport
            .send(ClientMessage {
                header: MessageHeader {
                    session: grant.session,
                    sequence: 0,
                },
                payload: ClientPayload::Leave,
            })
            .map_err(|reason| ClientFault::Transport(reason.to_string()))?;
        self.transport.disconnect();
        self.phase = ClientPhase::Closed(ClientClosure::LeftVoluntarily);
        Ok(())
    }
}

impl fmt::Debug for ClientSession {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("ClientSession")
            .field("phase", &self.phase)
            .field("grant", &self.transport.grant())
            .field("pending", &self.pending.len())
            .field("unacked", &self.unacked.len())
            .field("acked_through", &self.acked_through)
            .field("seen", &self.seen.len())
            .finish_non_exhaustive()
    }
}
