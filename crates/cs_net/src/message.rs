//! The in-session wire message vocabulary (F54-A).
//!
//! Spec: `specs/F54-modern-multiplayer-transport-and-authority-protocol.md`,
//! stage `### F54-A`; contract `docs/contracts/UI-NETWORK.md`.
//!
//! A [`SessionMessage`] is either direction of one sequenced in-session
//! packet. Every in-session message carries a [`MessageHeader`] with the
//! session epoch and the sender's sequence number ("Protocol messages carry
//! session epoch and sequence/tick"), and [`SessionMessage::expect_session`]
//! is the epoch check that rejects stale packets before their payload is
//! read.
//!
//! Authority is structural: everything the contract puts under server
//! ownership — spawning, physics truth, lifecycle, outcomes — exists only as
//! a [`ServerPayload`] variant, so a [`ClientPayload`] cannot express it, and
//! [`SessionMessage::verify_origin`] rejects a packet that claims the wrong
//! side at the codec boundary. [`Delivery`] records each payload's class:
//! lifecycle/rules/outcomes are [`Delivery::Reliable`] and idempotent by
//! [`cs_types::net::EventId`]; motion snapshots and input acknowledgments are
//! [`Delivery::Sequenced`] and may be dropped.
//!
//! Payloads are bounded typed records — every list and byte string is capped
//! by a [`crate::bounds`] constant and [`ClientMessage::validate`] /
//! [`ServerMessage::validate`] enforce those caps plus tick ordering. All
//! numeric fields are integers, and input axes arrive already quantized to
//! `i16` (`cs_types::input::AxisValue`), so finite numerics are structural:
//! no wire field can carry a NaN.
//!
//! The snapshot *payload* schema (quantized actor records and budgets) is
//! owned by F57-A (`snapshot.rs`); [`SnapshotFrame`] carries it as a bounded
//! byte string the envelope validates for size only, and nothing in this
//! stage interprets it.

use std::fmt;

use cs_types::Tick;
use cs_types::content::ContentId;
use cs_types::input::{Action, InputFrame, UiAction};
use cs_types::net::{ActorId, EventId, PeerId, SessionId};

use crate::bounds::{
    MAX_EDGES_PER_FRAME, MAX_INPUT_BATCH_SPAN_TICKS, MAX_INPUT_FRAMES_PER_PACKET,
    MAX_SNAPSHOT_BYTES,
};

/// How a payload must be delivered, per spec F54 non-negotiable behavior 4
/// ("reliable delivery for lifecycle/rules and sequenced snapshots for
/// motion"). The transport stage (F54-B) maps this onto real channels; the
/// classification is part of the message contract, not a transport
/// convenience.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Delivery {
    /// Must arrive exactly once: lifecycle, rules and outcome traffic.
    /// Application-level ids ([`EventId`]) still deduplicate, because
    /// reconnect and retry can replay a request even on a reliable channel
    /// (contract: "Reliable delivery does not replace application
    /// idempotency").
    Reliable,
    /// Sequenced and droppable: a newer packet supersedes an older one.
    Sequenced,
}

/// Which side of the session sent a packet.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Origin {
    /// The packet came from the session's server.
    Server,
    /// The packet came from one of the session's clients.
    Client,
}

/// The header every in-session message carries: the session epoch it belongs
/// to and the sender's own monotonic sequence number.
///
/// `sequence` orders one sender's packets and is the deduplication key for
/// that sender's traffic — a re-sent input packet keeps its sequence, so the
/// receiver can drop the replay instead of applying it twice (spec F54
/// AC02).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct MessageHeader {
    /// The session epoch this packet belongs to.
    pub session: SessionId,
    /// This sender's monotonic packet sequence.
    pub sequence: u32,
}

/// Why a wire record was refused by validation.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum WireError {
    /// The packet's session epoch is not the live one ("Epoch mismatch
    /// rejects stale packets").
    StaleSession {
        /// The live session.
        expected: SessionId,
        /// The epoch the packet carried.
        found: SessionId,
    },
    /// A packet arrived from a side that may never send it — the wire-level
    /// face of the authority table (a client cannot emit server-owned
    /// payloads).
    WrongOrigin {
        /// Where the packet claims to have come from.
        claimed: Origin,
        /// The payload kind it carried.
        payload: &'static str,
    },
    /// A required list was empty.
    Empty {
        /// The field that was empty.
        field: &'static str,
    },
    /// A bounded list exceeded its cap.
    TooMany {
        /// The field that overflowed.
        field: &'static str,
        /// The cap.
        max: usize,
        /// The offered count.
        len: usize,
    },
    /// A byte payload exceeded its cap.
    TooLarge {
        /// The field that overflowed.
        field: &'static str,
        /// The cap in bytes.
        max: usize,
        /// The offered size in bytes.
        len: usize,
    },
    /// Input frames were not in strictly increasing tick order.
    TicksOutOfOrder {
        /// The offending tick.
        tick: Tick,
        /// The tick it had to follow.
        previous: Tick,
    },
    /// An input batch spans more ticks than [`MAX_INPUT_BATCH_SPAN_TICKS`].
    SpanTooWide {
        /// The offered span.
        span: u64,
    },
    /// A UI action reached the wire. UI state is client-owned and is never
    /// a network input (`docs/contracts/UI-NETWORK.md`).
    UiActionOnWire {
        /// The action that must not travel.
        action: UiAction,
    },
}

impl fmt::Display for WireError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::StaleSession { expected, found } => write!(
                f,
                "stale packet: session {} is not the live session {}",
                found.get(),
                expected.get()
            ),
            Self::WrongOrigin { claimed, payload } => {
                write!(f, "a {claimed:?} peer must not send a {payload} payload")
            }
            Self::Empty { field } => write!(f, "{field} must not be empty"),
            Self::TooMany { field, max, len } => {
                write!(f, "{field} has {len} entries, max is {max}")
            }
            Self::TooLarge { field, max, len } => {
                write!(f, "{field} is {len} bytes, max is {max}")
            }
            Self::TicksOutOfOrder { tick, previous } => write!(
                f,
                "input tick {} does not follow tick {}",
                tick.0, previous.0
            ),
            Self::SpanTooWide { span } => write!(
                f,
                "input batch spans {span} ticks, max is {MAX_INPUT_BATCH_SPAN_TICKS}"
            ),
            Self::UiActionOnWire { action } => {
                write!(
                    f,
                    "UI action {action} is client-owned and never crosses the wire"
                )
            }
        }
    }
}

impl std::error::Error for WireError {}

/// One client's bounded, tick-stamped input for a stretch of simulation
/// ticks (spec F54 deliverable: "Clients submit bounded tick-stamped
/// inputs").
///
/// Each frame is the F22-A quantized [`InputFrame`] for one tick; the batch
/// is nonempty, capped at [`MAX_INPUT_FRAMES_PER_PACKET`], strictly
/// tick-ordered and spans at most [`MAX_INPUT_BATCH_SPAN_TICKS`]. The
/// packet's `MessageHeader::sequence` — not any field here — is what the
/// receiver deduplicates on, so a retransmitted batch can never apply twice.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct InputBatch {
    /// The recorded frames, oldest first.
    pub frames: Vec<InputFrame>,
}

impl InputBatch {
    /// Checks the packet bounds: nonempty, capped count and span, strictly
    /// increasing ticks, capped edges, no client-owned UI actions.
    ///
    /// Axis values need no numeric check: they are quantized `i16` samples by
    /// construction and `InputFrame::set_axis` already keeps one sample per
    /// command, so the only wire-visible input risks are count, order and
    /// the UI/authority split.
    ///
    /// # Errors
    ///
    /// [`WireError::Empty`], [`WireError::TooMany`],
    /// [`WireError::TicksOutOfOrder`], [`WireError::SpanTooWide`] and
    /// [`WireError::UiActionOnWire`].
    pub fn validate(&self) -> Result<(), WireError> {
        if self.frames.is_empty() {
            return Err(WireError::Empty {
                field: "input.frames",
            });
        }
        if self.frames.len() > MAX_INPUT_FRAMES_PER_PACKET {
            return Err(WireError::TooMany {
                field: "input.frames",
                max: MAX_INPUT_FRAMES_PER_PACKET,
                len: self.frames.len(),
            });
        }
        let mut previous: Option<Tick> = None;
        for frame in &self.frames {
            let tick = frame.frame_tick();
            if let Some(previous_tick) = previous
                && tick <= previous_tick
            {
                return Err(WireError::TicksOutOfOrder {
                    tick,
                    previous: previous_tick,
                });
            }
            previous = Some(tick);
            if frame.edges().len() > MAX_EDGES_PER_FRAME {
                return Err(WireError::TooMany {
                    field: "input.edges",
                    max: MAX_EDGES_PER_FRAME,
                    len: frame.edges().len(),
                });
            }
            for edge in frame.edges() {
                if let Action::Ui(action) = edge {
                    return Err(WireError::UiActionOnWire { action: *action });
                }
            }
        }
        let span = self.frames.last().map_or(0, |frame| {
            frame.frame_tick().0 - self.frames[0].frame_tick().0
        });
        if span > MAX_INPUT_BATCH_SPAN_TICKS {
            return Err(WireError::SpanTooWide { span });
        }
        Ok(())
    }
}

/// Why a peer left the session or the server cut it off.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum DisconnectReason {
    /// The peer asked to leave.
    Voluntary,
    /// The peer stopped responding.
    Timeout,
    /// The host ended the whole session.
    SessionEnded,
}

impl DisconnectReason {
    /// The stable label used in reports.
    pub const fn label(self) -> &'static str {
        match self {
            Self::Voluntary => "voluntary",
            Self::Timeout => "timeout",
            Self::SessionEnded => "session_ended",
        }
    }
}

/// How a session finished. The scoreboard and outcome detail are the
/// mission-rules domain; this reason only says the match closed.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum FinishReason {
    /// The mission program reached an outcome.
    Completed,
    /// The host aborted the session.
    Aborted,
}

impl FinishReason {
    /// The stable label used in reports.
    pub const fn label(self) -> &'static str {
        match self {
            Self::Completed => "completed",
            Self::Aborted => "aborted",
        }
    }
}

/// One reliable, idempotent semantic event: the lifecycle, membership and
/// authority facts the server publishes.
///
/// The kinds here are the *protocol-level* events F54 owns. Gameplay-domain
/// events (hits, objectives, dialogue, score lines) are declared by the
/// stages that own those domains and travel through this same envelope;
/// nothing may bypass the [`EventId`] dedup discipline.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum EventBody {
    /// A peer completed the handshake and joined the session.
    PeerJoined {
        /// The new peer.
        peer: PeerId,
    },
    /// A peer is gone.
    PeerLeft {
        /// The departed peer.
        peer: PeerId,
        /// Why it left.
        reason: DisconnectReason,
    },
    /// The server launched the match; simulation begins at `start_tick`.
    Launched {
        /// The tick the session's simulation starts on.
        start_tick: Tick,
    },
    /// The match ended.
    Finished {
        /// How it ended.
        reason: FinishReason,
    },
    /// The server allocated and spawned an actor under its authority.
    ActorSpawned {
        /// The actor id the server allocated.
        actor: ActorId,
        /// The catalog id of the blueprint it spawned from.
        blueprint: ContentId,
        /// The peer controlling it, when it is player-flown.
        owner: Option<PeerId>,
    },
    /// An actor left the world (despawn or mission removal). Destruction
    /// *semantics* are a damage-domain event owned by F29's consumers; this
    /// kind only says the id is dead.
    ActorRemoved {
        /// The actor that is gone.
        actor: ActorId,
    },
}

/// A reliable semantic event stamped with its deduplication id.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ReliableEvent {
    /// The event's `EventId(session, tick, producer, sequence)` identity:
    /// receivers deduplicate on this id, never on transport delivery.
    pub id: EventId,
    /// What happened.
    pub body: EventBody,
}

/// One sequenced motion snapshot: the server's physics truth for `tick`.
///
/// The `payload` is the quantized actor state whose record schema is owned
/// by F57-A (`snapshot.rs`); at this layer it is a bounded byte string — the
/// envelope caps it at [`MAX_SNAPSHOT_BYTES`] and does not interpret it.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SnapshotFrame {
    /// The simulation tick the snapshot describes.
    pub tick: Tick,
    /// The quantized actor records (schema: F57-A), capped at
    /// [`MAX_SNAPSHOT_BYTES`].
    pub payload: Vec<u8>,
}

/// What a client may send inside a session. Every variant is a *request* or
/// a farewell — no variant asserts server-owned state, so "no client can
/// spawn, score or launch" is a type fact, not a runtime check.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ClientPayload {
    /// Bounded tick-stamped quantized input.
    Input(InputBatch),
    /// The client is leaving the session.
    Leave,
}

/// What the server may send inside a session.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ServerPayload {
    /// Acknowledgment of processed client input: `through` is the highest
    /// client packet sequence the server has consumed ("Inputs have sequence
    /// acknowledgment", contract).
    InputAck {
        /// The highest consumed client sequence.
        through: u32,
    },
    /// A sequenced motion snapshot (droppable).
    Snapshot(SnapshotFrame),
    /// A reliable, idempotent semantic event.
    Event(ReliableEvent),
    /// The server closed the session or dropped this peer (reliable).
    Disconnect {
        /// Why.
        reason: DisconnectReason,
    },
}

impl ClientPayload {
    /// The delivery class this payload requires.
    pub const fn delivery(&self) -> Delivery {
        match self {
            // Latest input supersedes older input; the sequence number and
            // InputAck provide the dedup/retry story, not retransmission of
            // every frame.
            Self::Input(_) => Delivery::Sequenced,
            Self::Leave => Delivery::Reliable,
        }
    }
}

impl ServerPayload {
    /// The delivery class this payload requires.
    pub const fn delivery(&self) -> Delivery {
        match self {
            Self::InputAck { .. } | Self::Snapshot(_) => Delivery::Sequenced,
            Self::Event(_) | Self::Disconnect { .. } => Delivery::Reliable,
        }
    }
}

/// One sequenced in-session packet from client to server.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ClientMessage {
    /// Epoch and sender sequence.
    pub header: MessageHeader,
    /// What the client asks.
    pub payload: ClientPayload,
}

/// One sequenced in-session packet from server to client.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ServerMessage {
    /// Epoch and sender sequence.
    pub header: MessageHeader,
    /// What the server publishes.
    pub payload: ServerPayload,
}

/// One in-session packet in either direction: the codec boundary type.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum SessionMessage {
    /// A packet the server received from a client.
    ToServer(ClientMessage),
    /// A packet a client received from the server.
    ToClient(ServerMessage),
}

fn expect_session(header: &MessageHeader, session: SessionId) -> Result<(), WireError> {
    if header.session == session {
        Ok(())
    } else {
        Err(WireError::StaleSession {
            expected: session,
            found: header.session,
        })
    }
}

impl ClientMessage {
    /// Checks the payload's wire bounds.
    ///
    /// # Errors
    ///
    /// Any [`WireError`] the payload's own validation reports.
    pub fn validate(&self) -> Result<(), WireError> {
        match &self.payload {
            ClientPayload::Input(batch) => batch.validate(),
            ClientPayload::Leave => Ok(()),
        }
    }

    /// The delivery class the payload requires.
    pub const fn delivery(&self) -> Delivery {
        self.payload.delivery()
    }

    /// Refuses the packet when its session epoch is not `session`.
    ///
    /// # Errors
    ///
    /// [`WireError::StaleSession`].
    pub fn expect_session(&self, session: SessionId) -> Result<(), WireError> {
        expect_session(&self.header, session)
    }
}

impl ServerMessage {
    /// Checks the payload's wire bounds.
    ///
    /// # Errors
    ///
    /// [`WireError::TooLarge`] when a snapshot payload exceeds
    /// [`MAX_SNAPSHOT_BYTES`]; other payloads are bounded by construction.
    pub fn validate(&self) -> Result<(), WireError> {
        match &self.payload {
            ServerPayload::Snapshot(frame) => {
                if frame.payload.len() > MAX_SNAPSHOT_BYTES {
                    return Err(WireError::TooLarge {
                        field: "snapshot.payload",
                        max: MAX_SNAPSHOT_BYTES,
                        len: frame.payload.len(),
                    });
                }
                Ok(())
            }
            ServerPayload::InputAck { .. }
            | ServerPayload::Event(_)
            | ServerPayload::Disconnect { .. } => Ok(()),
        }
    }

    /// The delivery class the payload requires.
    pub const fn delivery(&self) -> Delivery {
        self.payload.delivery()
    }

    /// Refuses the packet when its session epoch is not `session`.
    ///
    /// # Errors
    ///
    /// [`WireError::StaleSession`].
    pub fn expect_session(&self, session: SessionId) -> Result<(), WireError> {
        expect_session(&self.header, session)
    }
}

impl SessionMessage {
    /// The delivery class the wrapped payload requires.
    pub const fn delivery(&self) -> Delivery {
        match self {
            Self::ToServer(message) => message.delivery(),
            Self::ToClient(message) => message.delivery(),
        }
    }

    /// The session epoch the packet carries.
    pub const fn session(&self) -> SessionId {
        match self {
            Self::ToServer(message) => message.header.session,
            Self::ToClient(message) => message.header.session,
        }
    }

    /// Checks the wrapped payload's wire bounds.
    ///
    /// # Errors
    ///
    /// Any [`WireError`] the payload's own validation reports.
    pub fn validate(&self) -> Result<(), WireError> {
        match self {
            Self::ToServer(message) => message.validate(),
            Self::ToClient(message) => message.validate(),
        }
    }

    /// Refuses the packet when its session epoch is not `session`.
    ///
    /// # Errors
    ///
    /// [`WireError::StaleSession`].
    pub fn expect_session(&self, session: SessionId) -> Result<(), WireError> {
        match self {
            Self::ToServer(message) => message.expect_session(session),
            Self::ToClient(message) => message.expect_session(session),
        }
    }

    /// The authority check at the codec boundary: a `ToServer` packet must
    /// have come from a [`Origin::Client`] and a `ToClient` packet from the
    /// [`Origin::Server`]. A packet that claims the wrong side is refused
    /// before its payload is trusted — a client can never inject a server
    /// payload because only [`ServerPayload`] expresses server authority.
    ///
    /// # Errors
    ///
    /// [`WireError::WrongOrigin`].
    pub fn verify_origin(&self, origin: Origin) -> Result<(), WireError> {
        match (self, origin) {
            (Self::ToServer(_), Origin::Client) | (Self::ToClient(_), Origin::Server) => Ok(()),
            (Self::ToServer(_), Origin::Server) => Err(WireError::WrongOrigin {
                claimed: Origin::Server,
                payload: "server-bound",
            }),
            (Self::ToClient(_), Origin::Client) => Err(WireError::WrongOrigin {
                claimed: Origin::Client,
                payload: "server-owned",
            }),
        }
    }
}
