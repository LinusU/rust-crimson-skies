//! The minimal synthetic wire fixture (F54-A).
//!
//! Spec: "Define typed inputs/outputs and a minimal synthetic fixture first."
//! These constructors build one self-consistent synthetic session: fixed
//! non-zero placeholder signatures (clearly *not* digests of original data),
//! a matching [`crate::compat::ClientHello`], and one valid in-session input
//! packet and event. Everything here is newly authored development content —
//! `SYNTHETIC` — and can never stand in for retail session data.

use cs_types::Tick;
use cs_types::content::{ContentId, ContentKind};
use cs_types::evidence::ContentHash;
use cs_types::input::{AxisValue, FlightCommand, InputFrame};
use cs_types::net::{EventId, PeerId, SessionId};

use crate::compat::{ClientHello, Compatibility, PROTOCOL_VERSION, SessionParameters};
use crate::message::{
    ClientMessage, ClientPayload, EventBody, InputBatch, MessageHeader, ReliableEvent,
};

/// The synthetic rules signature: a fixed marker, not a digest of anything.
pub const SYNTHETIC_RULES_SHA256: ContentHash = ContentHash::from_bytes([0x52; 32]);

/// The synthetic content signature: a fixed marker, not a digest of anything.
pub const SYNTHETIC_CONTENT_SHA256: ContentHash = ContentHash::from_bytes([0xC5; 32]);

/// The session id the synthetic fixture runs under.
pub const SYNTHETIC_SESSION: SessionId = match SessionId::new(1) {
    Some(id) => id,
    None => unreachable!(),
};

/// The peer id the synthetic fixture's client holds.
pub const SYNTHETIC_PEER: PeerId = match PeerId::new(1) {
    Some(id) => id,
    None => unreachable!(),
};

/// The synthetic compatibility signature: the two marker hashes and no mods.
pub fn synthetic_compatibility() -> Compatibility {
    Compatibility {
        rules_sha256: SYNTHETIC_RULES_SHA256,
        content_sha256: SYNTHETIC_CONTENT_SHA256,
        mods: Vec::new(),
    }
}

/// Synthetic session parameters a matching [`synthetic_hello`] clears.
pub fn synthetic_parameters() -> SessionParameters {
    SessionParameters {
        compatibility: synthetic_compatibility(),
    }
}

/// A client hello that [`crate::compat::evaluate_hello`] accepts against
/// [`synthetic_parameters`]. Tests build rejections by editing the public
/// fields of a clone.
pub fn synthetic_hello() -> ClientHello {
    ClientHello {
        protocol: PROTOCOL_VERSION,
        compatibility: synthetic_compatibility(),
    }
}

/// One valid in-session input packet for [`SYNTHETIC_SESSION`]: two
/// strictly-ordered frames, one holding a half-throttle sample and one edge.
pub fn synthetic_input_message(sequence: u32) -> ClientMessage {
    let mut first = InputFrame::new(Tick(10));
    first.set_axis(
        AxisValue::from_unit(FlightCommand::Throttle, 0.5)
            .expect("half throttle is a finite in-range axis"),
    );
    let mut second = InputFrame::new(Tick(11));
    second.push_edge(cs_types::input::Action::Flight(FlightCommand::FirePrimary));
    ClientMessage {
        header: MessageHeader {
            session: SYNTHETIC_SESSION,
            sequence,
        },
        payload: ClientPayload::Input(InputBatch {
            frames: vec![first, second],
        }),
    }
}

/// One valid membership event for [`SYNTHETIC_SESSION`]: `peer` joined.
pub fn synthetic_peer_joined(peer: PeerId, tick: Tick, sequence: u32) -> ReliableEvent {
    ReliableEvent {
        id: EventId {
            session: SYNTHETIC_SESSION,
            tick,
            producer: 0,
            sequence,
        },
        body: EventBody::PeerJoined { peer },
    }
}

/// A catalog id usable in synthetic fixtures.
pub fn synthetic_blueprint_id() -> ContentId {
    ContentId::from_source(ContentKind::Blueprint, "synthetic_zephyr")
        .expect("the synthetic key satisfies the id grammar")
}
