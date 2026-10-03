//! The bounded wire codec the pinned transport speaks (F54-B).
//!
//! Spec: `specs/F54-modern-multiplayer-transport-and-authority-protocol.md`,
//! stage `### F54-B`. Contract `docs/contracts/UI-NETWORK.md`: "Wire ids are
//! stable typed numeric/string keys with bounded lengths."
//!
//! This module turns the F54-A vocabulary into bytes and back. Every integer
//! is little-endian, every count is a `u16` checked against the matching
//! [`crate::bounds`] cap at decode time, and no field carries a float — the
//! "finite numeric fields" requirement stays structural. A decoded packet is
//! checked end to end: the buffer is refused when it exceeds
//! [`MAX_PACKET_BYTES`], every bounded list is refused past its cap, unknown
//! tags are refused, a truncated or trailing buffer is refused, zero ids are
//! refused (a nonzero id can never alias a live peer or session), and the
//! decoded message still runs its own `validate()` so wire-order and
//! payload rules ([`WireError`]) hold no matter which side sent the bytes.
//!
//! The packet kinds deliberately mirror the session lifecycle: a client may
//! send [`ClientPacket::Hello`] or [`ClientPacket::Message`], a server may
//! send [`ServerPacket::Reply`] or [`ServerPacket::Message`]. The pre-session
//! and in-session vocabularies never share a tag, so a decoder always knows
//! which grammar a packet must follow.
//!
//! All values here are newly authored engine design: this is *our* wire
//! format, not a claim about the original game's DirectPlay packets.

use std::fmt;

use cs_types::Tick;
use cs_types::content::ContentId;
use cs_types::evidence::ContentHash;
use cs_types::input::{Action, AxisValue, FlightCommand, InputFrame, UiAction};
use cs_types::net::{ActorId, EventId, PeerId, SessionId};

use crate::bounds::{
    MAX_EDGES_PER_FRAME, MAX_INPUT_FRAMES_PER_PACKET, MAX_MODS, MAX_PACKET_BYTES,
    MAX_SNAPSHOT_BYTES,
};
use crate::compat::{
    ClientHello, CompatError, Compatibility, HandshakeReject, HelloReply, ProtocolVersion,
    SessionGrant,
};
use crate::message::{
    ClientMessage, ClientPayload, DisconnectReason, EventBody, FinishReason, InputBatch,
    MessageHeader, ReliableEvent, ServerMessage, ServerPayload, SnapshotFrame, WireError,
};

/// The widest canonical `namespace/key` text a [`ContentId`] may take on the
/// wire: the key cap plus the longest namespace label and the separator.
/// Anything longer is refused before the grammar runs.
pub const MAX_CONTENT_ID_WIRE_BYTES: usize = 256;

/// The packet kind a client sends on the wire.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ClientPacket {
    /// The pre-session offer (`crate::compat`).
    Hello(ClientHello),
    /// An in-session sequenced packet.
    Message(ClientMessage),
}

/// The packet kind a server sends on the wire.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ServerPacket {
    /// The pre-session answer (`crate::compat`).
    Reply(HelloReply),
    /// An in-session sequenced packet.
    Message(ServerMessage),
}

/// Why a byte buffer could not be decoded, or a record could not be encoded.
///
/// The decode-side variants double as the transport's threat input: an
/// oversized buffer or count is an `OversizedMessage` abuse case
/// ([`CodecError::is_oversized`]), everything else is a malformed packet.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum CodecError {
    /// The buffer ended before the named field could be read.
    Truncated {
        /// The field that ran out of bytes.
        field: &'static str,
        /// How many bytes it needed.
        needed: usize,
        /// How many were left.
        remaining: usize,
    },
    /// A packet or a length-prefixed field exceeded its cap.
    TooLarge {
        /// The field that overflowed.
        field: &'static str,
        /// The cap.
        max: usize,
        /// The offered size.
        len: usize,
    },
    /// A count field exceeded its cap.
    TooMany {
        /// The field that overflowed.
        field: &'static str,
        /// The cap.
        max: usize,
        /// The offered count.
        len: usize,
    },
    /// A tag did not name a known variant or enum member.
    UnknownTag {
        /// The field the tag was read for.
        field: &'static str,
        /// The unrecognized value.
        tag: u8,
    },
    /// Bytes remained after the record was fully decoded.
    Trailing {
        /// How many undecoded bytes followed the record.
        len: usize,
    },
    /// An id field carried zero, which never names a live peer or session.
    ZeroId {
        /// The field that was zero.
        field: &'static str,
    },
    /// A text field was not valid UTF-8 or a valid [`ContentId`].
    BadText {
        /// The field that was malformed.
        field: &'static str,
    },
    /// The packet kind was structurally legal but wrong for this decoder's
    /// point in the session (e.g. a reply where a hello belongs).
    UnexpectedKind {
        /// The kind this decoder expected.
        expected: &'static str,
        /// The kind it found.
        found: &'static str,
    },
    /// The decoded message failed its own wire validation.
    Wire(WireError),
    /// The compatibility record failed its own bounds validation.
    Compat(CompatError),
}

impl CodecError {
    /// Whether this failure belongs to the `OversizedMessage` threat class
    /// (disconnect) rather than `MalformedMessage` (absorb).
    #[must_use]
    pub const fn is_oversized(&self) -> bool {
        matches!(self, Self::TooLarge { .. } | Self::TooMany { .. })
    }
}

impl fmt::Display for CodecError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Truncated {
                field,
                needed,
                remaining,
            } => write!(f, "{field} needs {needed} bytes, only {remaining} remain"),
            Self::TooLarge { field, max, len } => {
                write!(f, "{field} is {len} bytes, max is {max}")
            }
            Self::TooMany { field, max, len } => {
                write!(f, "{field} has {len} entries, max is {max}")
            }
            Self::UnknownTag { field, tag } => {
                write!(f, "{field} has unknown tag {tag}")
            }
            Self::Trailing { len } => {
                write!(f, "{len} undecoded bytes trail the packet")
            }
            Self::ZeroId { field } => {
                write!(f, "{field} is zero, which never names a live id")
            }
            Self::BadText { field } => write!(f, "{field} is not valid text"),
            Self::UnexpectedKind { expected, found } => {
                write!(f, "expected a {expected} packet, found {found}")
            }
            Self::Wire(reason) => write!(f, "invalid message: {reason}"),
            Self::Compat(reason) => write!(f, "invalid signature: {reason}"),
        }
    }
}

impl std::error::Error for CodecError {}

impl From<WireError> for CodecError {
    fn from(reason: WireError) -> Self {
        Self::Wire(reason)
    }
}

impl From<CompatError> for CodecError {
    fn from(reason: CompatError) -> Self {
        Self::Compat(reason)
    }
}

// Packet kind tags. The pre-session and in-session vocabularies share no tag.
const TAG_HELLO: u8 = 0;
const TAG_HELLO_REPLY: u8 = 1;
const TAG_CLIENT_MESSAGE: u8 = 2;
const TAG_SERVER_MESSAGE: u8 = 3;

/// A bounded reader over one packet buffer.
struct Reader<'a> {
    buf: &'a [u8],
    pos: usize,
}

impl<'a> Reader<'a> {
    fn new(buf: &'a [u8]) -> Self {
        Self { buf, pos: 0 }
    }

    fn remaining(&self) -> usize {
        self.buf.len() - self.pos
    }

    fn take(&mut self, field: &'static str, len: usize) -> Result<&'a [u8], CodecError> {
        if self.remaining() < len {
            return Err(CodecError::Truncated {
                field,
                needed: len,
                remaining: self.remaining(),
            });
        }
        let bytes = &self.buf[self.pos..self.pos + len];
        self.pos += len;
        Ok(bytes)
    }

    fn u8(&mut self, field: &'static str) -> Result<u8, CodecError> {
        Ok(self.take(field, 1)?[0])
    }

    fn u16(&mut self, field: &'static str) -> Result<u16, CodecError> {
        let bytes = self.take(field, 2)?;
        Ok(u16::from_le_bytes([bytes[0], bytes[1]]))
    }

    fn u32(&mut self, field: &'static str) -> Result<u32, CodecError> {
        let bytes = self.take(field, 4)?;
        Ok(u32::from_le_bytes(bytes.try_into().expect("four bytes")))
    }

    fn u64(&mut self, field: &'static str) -> Result<u64, CodecError> {
        let bytes = self.take(field, 8)?;
        Ok(u64::from_le_bytes(bytes.try_into().expect("eight bytes")))
    }

    fn i16(&mut self, field: &'static str) -> Result<i16, CodecError> {
        let bytes = self.take(field, 2)?;
        Ok(i16::from_le_bytes([bytes[0], bytes[1]]))
    }

    /// A `u16` count, refused when it exceeds `max`.
    fn count(&mut self, field: &'static str, max: usize) -> Result<usize, CodecError> {
        let len = usize::from(self.u16(field)?);
        if len > max {
            return Err(CodecError::TooMany { field, max, len });
        }
        Ok(len)
    }

    /// A `u16`-length byte string, refused when it exceeds `max`.
    fn bytes(&mut self, field: &'static str, max: usize) -> Result<&'a [u8], CodecError> {
        let len = usize::from(self.u16(field)?);
        if len > max {
            return Err(CodecError::TooLarge { field, max, len });
        }
        self.take(field, len)
    }

    /// A `u32`-length byte string, refused when it exceeds `max`.
    fn bytes32(&mut self, field: &'static str, max: usize) -> Result<&'a [u8], CodecError> {
        let len = self.u32(field)? as usize;
        if len > max {
            return Err(CodecError::TooLarge { field, max, len });
        }
        self.take(field, len)
    }

    fn text(&mut self, field: &'static str, max: usize) -> Result<String, CodecError> {
        let bytes = self.bytes(field, max)?;
        String::from_utf8(bytes.to_vec()).map_err(|_| CodecError::BadText { field })
    }

    fn done(&self) -> Result<(), CodecError> {
        if self.remaining() == 0 {
            Ok(())
        } else {
            Err(CodecError::Trailing {
                len: self.remaining(),
            })
        }
    }
}

/// A bounded writer that refuses to emit a packet past [`MAX_PACKET_BYTES`].
struct Writer(Vec<u8>);

impl Writer {
    fn new() -> Self {
        Self(Vec::new())
    }

    fn u8(&mut self, value: u8) {
        self.0.push(value);
    }

    fn u16(&mut self, value: u16) {
        self.0.extend_from_slice(&value.to_le_bytes());
    }

    fn u32(&mut self, value: u32) {
        self.0.extend_from_slice(&value.to_le_bytes());
    }

    fn u64(&mut self, value: u64) {
        self.0.extend_from_slice(&value.to_le_bytes());
    }

    fn i16(&mut self, value: i16) {
        self.0.extend_from_slice(&value.to_le_bytes());
    }

    fn bytes(&mut self, bytes: &[u8]) {
        self.0.extend_from_slice(bytes);
    }

    /// A `u16` length plus bytes. Callers keep `bytes` inside the `u16` space
    /// or the packet cap; the cap check in `finish` is the enforcement.
    fn counted_bytes(&mut self, bytes: &[u8]) {
        self.u16(bytes.len() as u16);
        self.bytes(bytes);
    }

    fn finish(self, field: &'static str) -> Result<Vec<u8>, CodecError> {
        if self.0.len() > MAX_PACKET_BYTES {
            return Err(CodecError::TooLarge {
                field,
                max: MAX_PACKET_BYTES,
                len: self.0.len(),
            });
        }
        Ok(self.0)
    }
}

fn put_count(w: &mut Writer, len: usize) {
    w.u16(len as u16);
}

fn session_id(r: &mut Reader<'_>, field: &'static str) -> Result<SessionId, CodecError> {
    SessionId::new(r.u64(field)?).ok_or(CodecError::ZeroId { field })
}

fn peer_id(r: &mut Reader<'_>, field: &'static str) -> Result<PeerId, CodecError> {
    PeerId::new(r.u16(field)?).ok_or(CodecError::ZeroId { field })
}

fn content_id(r: &mut Reader<'_>, field: &'static str) -> Result<ContentId, CodecError> {
    let text = r.text(field, MAX_CONTENT_ID_WIRE_BYTES)?;
    ContentId::parse(&text).map_err(|_| CodecError::BadText { field })
}

fn tick(r: &mut Reader<'_>, field: &'static str) -> Result<Tick, CodecError> {
    Ok(Tick(r.u64(field)?))
}

fn protocol_version(
    r: &mut Reader<'_>,
    field: &'static str,
) -> Result<ProtocolVersion, CodecError> {
    ProtocolVersion::new(r.u16(field)?).ok_or(CodecError::ZeroId { field })
}

fn put_content_id(w: &mut Writer, id: &ContentId) {
    w.counted_bytes(id.as_str().as_bytes());
}

fn put_hash(w: &mut Writer, hash: &ContentHash) {
    w.bytes(hash.as_bytes());
}

fn hash(r: &mut Reader<'_>, field: &'static str) -> Result<ContentHash, CodecError> {
    let bytes = r.take(field, 32)?;
    Ok(ContentHash::from_bytes(
        bytes.try_into().expect("thirty-two bytes"),
    ))
}

fn compatibility(r: &mut Reader<'_>) -> Result<Compatibility, CodecError> {
    let rules_sha256 = hash(r, "compat.rules_sha256")?;
    let content_sha256 = hash(r, "compat.content_sha256")?;
    let mod_count = r.count("compat.mods", MAX_MODS)?;
    let mut mods = Vec::with_capacity(mod_count);
    for _ in 0..mod_count {
        mods.push(content_id(r, "compat.mods[]")?);
    }
    Ok(Compatibility {
        rules_sha256,
        content_sha256,
        mods,
    })
}

fn put_compatibility(w: &mut Writer, compat: &Compatibility) {
    put_hash(w, &compat.rules_sha256);
    put_hash(w, &compat.content_sha256);
    put_count(w, compat.mods.len());
    for id in &compat.mods {
        put_content_id(w, id);
    }
}

fn client_hello(r: &mut Reader<'_>) -> Result<ClientHello, CodecError> {
    let protocol = protocol_version(r, "hello.protocol")?;
    let compatibility = compatibility(r)?;
    Ok(ClientHello {
        protocol,
        compatibility,
    })
}

fn put_client_hello(w: &mut Writer, hello: &ClientHello) {
    w.u16(hello.protocol.get());
    put_compatibility(w, &hello.compatibility);
}

fn compat_error(r: &mut Reader<'_>) -> Result<CompatError, CodecError> {
    match r.u8("compat_error.tag")? {
        0 => Ok(CompatError::TooManyMods {
            len: r.u32("compat_error.len")? as usize,
        }),
        1 => Ok(CompatError::DuplicateMod {
            id: content_id(r, "compat_error.id")?,
        }),
        tag => Err(CodecError::UnknownTag {
            field: "compat_error.tag",
            tag,
        }),
    }
}

fn put_compat_error(w: &mut Writer, error: &CompatError) {
    match error {
        CompatError::TooManyMods { len } => {
            w.u8(0);
            w.u32(*len as u32);
        }
        CompatError::DuplicateMod { id } => {
            w.u8(1);
            put_content_id(w, id);
        }
    }
}

fn content_id_list(r: &mut Reader<'_>, field: &'static str) -> Result<Vec<ContentId>, CodecError> {
    let count = r.count(field, MAX_MODS)?;
    let mut ids = Vec::with_capacity(count);
    for _ in 0..count {
        ids.push(content_id(r, field)?);
    }
    Ok(ids)
}

fn put_content_id_list(w: &mut Writer, ids: &[ContentId]) {
    put_count(w, ids.len());
    for id in ids {
        put_content_id(w, id);
    }
}

fn handshake_reject(r: &mut Reader<'_>) -> Result<HandshakeReject, CodecError> {
    match r.u8("reject.tag")? {
        0 => Ok(HandshakeReject::MalformedSignature(compat_error(r)?)),
        1 => {
            let offered = protocol_version(r, "reject.offered")?;
            let supported = protocol_version(r, "reject.supported")?;
            Ok(HandshakeReject::UnsupportedProtocol { offered, supported })
        }
        2 => {
            let expected = hash(r, "reject.rules.expected")?;
            let offered = hash(r, "reject.rules.offered")?;
            Ok(HandshakeReject::RulesMismatch { expected, offered })
        }
        3 => {
            let expected = hash(r, "reject.content.expected")?;
            let offered = hash(r, "reject.content.offered")?;
            Ok(HandshakeReject::ContentMismatch { expected, offered })
        }
        4 => {
            let missing = content_id_list(r, "reject.mods.missing")?;
            let unexpected = content_id_list(r, "reject.mods.unexpected")?;
            Ok(HandshakeReject::ModSetMismatch {
                missing,
                unexpected,
            })
        }
        5 => Ok(HandshakeReject::SessionFull {
            max: r.u32("reject.max")? as usize,
        }),
        tag => Err(CodecError::UnknownTag {
            field: "reject.tag",
            tag,
        }),
    }
}

fn put_handshake_reject(w: &mut Writer, reject: &HandshakeReject) {
    match reject {
        HandshakeReject::MalformedSignature(error) => {
            w.u8(0);
            put_compat_error(w, error);
        }
        HandshakeReject::UnsupportedProtocol { offered, supported } => {
            w.u8(1);
            w.u16(offered.get());
            w.u16(supported.get());
        }
        HandshakeReject::RulesMismatch { expected, offered } => {
            w.u8(2);
            put_hash(w, expected);
            put_hash(w, offered);
        }
        HandshakeReject::ContentMismatch { expected, offered } => {
            w.u8(3);
            put_hash(w, expected);
            put_hash(w, offered);
        }
        HandshakeReject::ModSetMismatch {
            missing,
            unexpected,
        } => {
            w.u8(4);
            put_content_id_list(w, missing);
            put_content_id_list(w, unexpected);
        }
        HandshakeReject::SessionFull { max } => {
            w.u8(5);
            w.u32(*max as u32);
        }
    }
}

fn hello_reply(r: &mut Reader<'_>) -> Result<HelloReply, CodecError> {
    match r.u8("reply.tag")? {
        0 => {
            let session = session_id(r, "reply.session")?;
            let peer = peer_id(r, "reply.peer")?;
            Ok(HelloReply::Welcome(SessionGrant { session, peer }))
        }
        1 => Ok(HelloReply::Rejected(handshake_reject(r)?)),
        tag => Err(CodecError::UnknownTag {
            field: "reply.tag",
            tag,
        }),
    }
}

fn put_hello_reply(w: &mut Writer, reply: &HelloReply) {
    match reply {
        HelloReply::Welcome(grant) => {
            w.u8(0);
            w.u64(grant.session.get());
            w.u16(grant.peer.get());
        }
        HelloReply::Rejected(reject) => {
            w.u8(1);
            put_handshake_reject(w, reject);
        }
    }
}

fn action(r: &mut Reader<'_>, field: &'static str) -> Result<Action, CodecError> {
    match r.u8(field)? {
        0 => {
            let index = usize::from(r.u8(field)?);
            FlightCommand::ALL
                .get(index)
                .copied()
                .map(Action::Flight)
                .ok_or(CodecError::UnknownTag {
                    field,
                    tag: index as u8,
                })
        }
        1 => {
            let index = usize::from(r.u8(field)?);
            UiAction::ALL
                .get(index)
                .copied()
                .map(Action::Ui)
                .ok_or(CodecError::UnknownTag {
                    field,
                    tag: index as u8,
                })
        }
        tag => Err(CodecError::UnknownTag { field, tag }),
    }
}

fn put_action(w: &mut Writer, action: Action) {
    match action {
        Action::Flight(command) => {
            let index = FlightCommand::ALL
                .iter()
                .position(|candidate| *candidate == command)
                .expect("every FlightCommand is in ALL");
            w.u8(0);
            w.u8(index as u8);
        }
        Action::Ui(ui_action) => {
            let index = UiAction::ALL
                .iter()
                .position(|candidate| *candidate == ui_action)
                .expect("every UiAction is in ALL");
            w.u8(1);
            w.u8(index as u8);
        }
    }
}

fn axis_value(r: &mut Reader<'_>, field: &'static str) -> Result<AxisValue, CodecError> {
    let index = usize::from(r.u8(field)?);
    let command = FlightCommand::CONTINUOUS
        .get(index)
        .copied()
        .ok_or(CodecError::UnknownTag {
            field,
            tag: index as u8,
        })?;
    let quantized = r.i16(field)?;
    Ok(AxisValue::from_quantized(command, quantized)
        .expect("CONTINUOUS only holds continuous commands"))
}

fn put_axis_value(w: &mut Writer, axis: AxisValue) {
    let index = FlightCommand::CONTINUOUS
        .iter()
        .position(|candidate| *candidate == axis.command())
        .expect("an AxisValue is always a continuous command");
    w.u8(index as u8);
    w.i16(axis.quantized());
}

fn input_frame(r: &mut Reader<'_>) -> Result<InputFrame, CodecError> {
    let frame_tick = tick(r, "frame.tick")?;
    let mut frame = InputFrame::new(frame_tick);
    let axis_count = r.count("frame.axes", FlightCommand::CONTINUOUS.len())?;
    for _ in 0..axis_count {
        let axis = axis_value(r, "frame.axes[]")?;
        frame.set_axis(axis);
    }
    let edge_count = r.count("frame.edges", MAX_EDGES_PER_FRAME)?;
    for _ in 0..edge_count {
        let edge = action(r, "frame.edges[]")?;
        frame.push_edge(edge);
    }
    Ok(frame)
}

fn put_input_frame(w: &mut Writer, frame: &InputFrame) {
    w.u64(frame.frame_tick().0);
    put_count(w, frame.axes().len());
    for axis in frame.axes() {
        put_axis_value(w, *axis);
    }
    put_count(w, frame.edges().len());
    for edge in frame.edges() {
        put_action(w, *edge);
    }
}

fn input_batch(r: &mut Reader<'_>) -> Result<InputBatch, CodecError> {
    let frame_count = r.count("input.frames", MAX_INPUT_FRAMES_PER_PACKET)?;
    let mut frames = Vec::with_capacity(frame_count);
    for _ in 0..frame_count {
        frames.push(input_frame(r)?);
    }
    Ok(InputBatch { frames })
}

fn put_input_batch(w: &mut Writer, batch: &InputBatch) {
    put_count(w, batch.frames.len());
    for frame in &batch.frames {
        put_input_frame(w, frame);
    }
}

fn disconnect_reason(
    r: &mut Reader<'_>,
    field: &'static str,
) -> Result<DisconnectReason, CodecError> {
    match r.u8(field)? {
        0 => Ok(DisconnectReason::Voluntary),
        1 => Ok(DisconnectReason::Timeout),
        2 => Ok(DisconnectReason::SessionEnded),
        tag => Err(CodecError::UnknownTag { field, tag }),
    }
}

fn put_disconnect_reason(w: &mut Writer, reason: DisconnectReason) {
    w.u8(match reason {
        DisconnectReason::Voluntary => 0,
        DisconnectReason::Timeout => 1,
        DisconnectReason::SessionEnded => 2,
    });
}

fn finish_reason(r: &mut Reader<'_>, field: &'static str) -> Result<FinishReason, CodecError> {
    match r.u8(field)? {
        0 => Ok(FinishReason::Completed),
        1 => Ok(FinishReason::Aborted),
        tag => Err(CodecError::UnknownTag { field, tag }),
    }
}

fn put_finish_reason(w: &mut Writer, reason: FinishReason) {
    w.u8(match reason {
        FinishReason::Completed => 0,
        FinishReason::Aborted => 1,
    });
}

fn actor_id(r: &mut Reader<'_>, field: &'static str) -> Result<ActorId, CodecError> {
    let session = session_id(r, field)?;
    let serial = r.u64(field)?;
    Ok(ActorId { session, serial })
}

fn put_actor_id(w: &mut Writer, actor: &ActorId) {
    w.u64(actor.session.get());
    w.u64(actor.serial);
}

fn event_id(r: &mut Reader<'_>, field: &'static str) -> Result<EventId, CodecError> {
    let session = session_id(r, field)?;
    let tick = Tick(r.u64(field)?);
    let producer = r.u32(field)?;
    let sequence = r.u32(field)?;
    Ok(EventId {
        session,
        tick,
        producer,
        sequence,
    })
}

fn put_event_id(w: &mut Writer, id: &EventId) {
    w.u64(id.session.get());
    w.u64(id.tick.0);
    w.u32(id.producer);
    w.u32(id.sequence);
}

fn event_body(r: &mut Reader<'_>) -> Result<EventBody, CodecError> {
    match r.u8("event.tag")? {
        0 => Ok(EventBody::PeerJoined {
            peer: peer_id(r, "event.peer")?,
        }),
        1 => {
            let peer = peer_id(r, "event.peer")?;
            let reason = disconnect_reason(r, "event.reason")?;
            Ok(EventBody::PeerLeft { peer, reason })
        }
        2 => Ok(EventBody::Launched {
            start_tick: tick(r, "event.start_tick")?,
        }),
        3 => Ok(EventBody::Finished {
            reason: finish_reason(r, "event.reason")?,
        }),
        4 => {
            let actor = actor_id(r, "event.actor")?;
            let blueprint = content_id(r, "event.blueprint")?;
            let owner = match r.u8("event.owner")? {
                0 => None,
                1 => Some(peer_id(r, "event.owner")?),
                tag => {
                    return Err(CodecError::UnknownTag {
                        field: "event.owner",
                        tag,
                    });
                }
            };
            Ok(EventBody::ActorSpawned {
                actor,
                blueprint,
                owner,
            })
        }
        5 => Ok(EventBody::ActorRemoved {
            actor: actor_id(r, "event.actor")?,
        }),
        tag => Err(CodecError::UnknownTag {
            field: "event.tag",
            tag,
        }),
    }
}

fn put_event_body(w: &mut Writer, body: &EventBody) {
    match body {
        EventBody::PeerJoined { peer } => {
            w.u8(0);
            w.u16(peer.get());
        }
        EventBody::PeerLeft { peer, reason } => {
            w.u8(1);
            w.u16(peer.get());
            put_disconnect_reason(w, *reason);
        }
        EventBody::Launched { start_tick } => {
            w.u8(2);
            w.u64(start_tick.0);
        }
        EventBody::Finished { reason } => {
            w.u8(3);
            put_finish_reason(w, *reason);
        }
        EventBody::ActorSpawned {
            actor,
            blueprint,
            owner,
        } => {
            w.u8(4);
            put_actor_id(w, actor);
            put_content_id(w, blueprint);
            match owner {
                Some(peer) => {
                    w.u8(1);
                    w.u16(peer.get());
                }
                None => w.u8(0),
            }
        }
        EventBody::ActorRemoved { actor } => {
            w.u8(5);
            put_actor_id(w, actor);
        }
    }
}

fn reliable_event(r: &mut Reader<'_>) -> Result<ReliableEvent, CodecError> {
    let id = event_id(r, "event.id")?;
    let body = event_body(r)?;
    Ok(ReliableEvent { id, body })
}

fn put_reliable_event(w: &mut Writer, event: &ReliableEvent) {
    put_event_id(w, &event.id);
    put_event_body(w, &event.body);
}

fn client_payload(r: &mut Reader<'_>) -> Result<ClientPayload, CodecError> {
    match r.u8("client.tag")? {
        0 => Ok(ClientPayload::Input(input_batch(r)?)),
        1 => Ok(ClientPayload::Leave),
        tag => Err(CodecError::UnknownTag {
            field: "client.tag",
            tag,
        }),
    }
}

fn put_client_payload(w: &mut Writer, payload: &ClientPayload) {
    match payload {
        ClientPayload::Input(batch) => {
            w.u8(0);
            put_input_batch(w, batch);
        }
        ClientPayload::Leave => w.u8(1),
    }
}

fn snapshot_frame(r: &mut Reader<'_>) -> Result<SnapshotFrame, CodecError> {
    let tick = tick(r, "snapshot.tick")?;
    let payload = r.bytes32("snapshot.payload", MAX_SNAPSHOT_BYTES)?.to_vec();
    Ok(SnapshotFrame { tick, payload })
}

fn put_snapshot_frame(w: &mut Writer, frame: &SnapshotFrame) {
    w.u64(frame.tick.0);
    w.u32(frame.payload.len() as u32);
    w.bytes(&frame.payload);
}

fn server_payload(r: &mut Reader<'_>) -> Result<ServerPayload, CodecError> {
    match r.u8("server.tag")? {
        0 => Ok(ServerPayload::InputAck {
            through: r.u32("ack.through")?,
        }),
        1 => Ok(ServerPayload::Snapshot(snapshot_frame(r)?)),
        2 => Ok(ServerPayload::Event(reliable_event(r)?)),
        3 => Ok(ServerPayload::Disconnect {
            reason: disconnect_reason(r, "disconnect.reason")?,
        }),
        tag => Err(CodecError::UnknownTag {
            field: "server.tag",
            tag,
        }),
    }
}

fn put_server_payload(w: &mut Writer, payload: &ServerPayload) {
    match payload {
        ServerPayload::InputAck { through } => {
            w.u8(0);
            w.u32(*through);
        }
        ServerPayload::Snapshot(frame) => {
            w.u8(1);
            put_snapshot_frame(w, frame);
        }
        ServerPayload::Event(event) => {
            w.u8(2);
            put_reliable_event(w, event);
        }
        ServerPayload::Disconnect { reason } => {
            w.u8(3);
            put_disconnect_reason(w, *reason);
        }
    }
}

fn header(r: &mut Reader<'_>) -> Result<MessageHeader, CodecError> {
    let session = session_id(r, "header.session")?;
    let sequence = r.u32("header.sequence")?;
    Ok(MessageHeader { session, sequence })
}

fn put_header(w: &mut Writer, header: &MessageHeader) {
    w.u64(header.session.get());
    w.u32(header.sequence);
}

/// Encodes one client-bound packet, refusing to emit an invalid or oversized
/// record.
///
/// # Errors
///
/// [`CodecError::Wire`] when the message fails its own validation and
/// [`CodecError::TooLarge`] when the encoded packet would exceed
/// [`MAX_PACKET_BYTES`].
pub fn encode_client_packet(packet: &ClientPacket) -> Result<Vec<u8>, CodecError> {
    match packet {
        ClientPacket::Hello(hello) => {
            hello.compatibility.validate()?;
            let mut w = Writer::new();
            w.u8(TAG_HELLO);
            put_client_hello(&mut w, hello);
            w.finish("client_packet")
        }
        ClientPacket::Message(message) => encode_client_message(message),
    }
}

/// Decodes one client-bound packet buffer.
///
/// # Errors
///
/// [`CodecError::TooLarge`] when the buffer exceeds [`MAX_PACKET_BYTES`], or
/// the first decode failure inside it; the decoded [`ClientMessage`] still
/// runs its own `validate()`.
pub fn decode_client_packet(buf: &[u8]) -> Result<ClientPacket, CodecError> {
    if buf.len() > MAX_PACKET_BYTES {
        return Err(CodecError::TooLarge {
            field: "client_packet",
            max: MAX_PACKET_BYTES,
            len: buf.len(),
        });
    }
    let mut r = Reader::new(buf);
    let packet = match r.u8("packet.tag")? {
        TAG_HELLO => ClientPacket::Hello(client_hello(&mut r)?),
        TAG_CLIENT_MESSAGE => {
            let header = header(&mut r)?;
            let payload = client_payload(&mut r)?;
            let message = ClientMessage { header, payload };
            message.validate()?;
            ClientPacket::Message(message)
        }
        TAG_HELLO_REPLY => {
            return Err(CodecError::UnexpectedKind {
                expected: "client",
                found: "hello_reply",
            });
        }
        TAG_SERVER_MESSAGE => {
            return Err(CodecError::UnexpectedKind {
                expected: "client",
                found: "server_message",
            });
        }
        tag => {
            return Err(CodecError::UnknownTag {
                field: "packet.tag",
                tag,
            });
        }
    };
    r.done()?;
    Ok(packet)
}

/// Encodes one server-bound packet, refusing to emit an invalid or oversized
/// record.
///
/// # Errors
///
/// [`CodecError::Wire`] when the message fails its own validation and
/// [`CodecError::TooLarge`] when the encoded packet would exceed
/// [`MAX_PACKET_BYTES`].
pub fn encode_server_packet(packet: &ServerPacket) -> Result<Vec<u8>, CodecError> {
    match packet {
        ServerPacket::Reply(reply) => {
            let mut w = Writer::new();
            w.u8(TAG_HELLO_REPLY);
            put_hello_reply(&mut w, reply);
            w.finish("server_packet")
        }
        ServerPacket::Message(message) => encode_server_message(message),
    }
}

/// Encodes one in-session client packet without wrapping it in
/// [`ClientPacket`].
///
/// # Errors
///
/// As [`encode_client_packet`].
pub fn encode_client_message(message: &ClientMessage) -> Result<Vec<u8>, CodecError> {
    message.validate()?;
    let mut w = Writer::new();
    w.u8(TAG_CLIENT_MESSAGE);
    put_header(&mut w, &message.header);
    put_client_payload(&mut w, &message.payload);
    w.finish("client_packet")
}

/// Encodes one in-session server packet without wrapping it in
/// [`ServerPacket`].
///
/// # Errors
///
/// As [`encode_server_packet`].
pub fn encode_server_message(message: &ServerMessage) -> Result<Vec<u8>, CodecError> {
    message.validate()?;
    let mut w = Writer::new();
    w.u8(TAG_SERVER_MESSAGE);
    put_header(&mut w, &message.header);
    put_server_payload(&mut w, &message.payload);
    w.finish("server_packet")
}

/// Decodes one server-bound packet buffer.
///
/// # Errors
///
/// [`CodecError::TooLarge`] when the buffer exceeds [`MAX_PACKET_BYTES`], or
/// the first decode failure inside it; the decoded [`ServerMessage`] still
/// runs its own `validate()`.
pub fn decode_server_packet(buf: &[u8]) -> Result<ServerPacket, CodecError> {
    if buf.len() > MAX_PACKET_BYTES {
        return Err(CodecError::TooLarge {
            field: "server_packet",
            max: MAX_PACKET_BYTES,
            len: buf.len(),
        });
    }
    let mut r = Reader::new(buf);
    let packet = match r.u8("packet.tag")? {
        TAG_HELLO_REPLY => ServerPacket::Reply(hello_reply(&mut r)?),
        TAG_SERVER_MESSAGE => {
            let header = header(&mut r)?;
            let payload = server_payload(&mut r)?;
            let message = ServerMessage { header, payload };
            message.validate()?;
            ServerPacket::Message(message)
        }
        TAG_HELLO => {
            return Err(CodecError::UnexpectedKind {
                expected: "server",
                found: "hello",
            });
        }
        TAG_CLIENT_MESSAGE => {
            return Err(CodecError::UnexpectedKind {
                expected: "server",
                found: "client_message",
            });
        }
        tag => {
            return Err(CodecError::UnknownTag {
                field: "packet.tag",
                tag,
            });
        }
    };
    r.done()?;
    Ok(packet)
}
