//! Acceptance scenario F54-B: one pinned transport and the handshake over it.
//!
//! Spec: `specs/F54-modern-multiplayer-transport-and-authority-protocol.md`,
//! stage `### F54-B`. Minimum scenario (spec F54 AC02): duplicate/out-of-order
//! inputs cannot duplicate fire or score — proven here end to end over real
//! UDP loopback packets that run the production codec, handshake and session
//! gate. Task test prefix: `accept_f54_b_`.
//!
//! Every test drives production code in `cs_net::codec` and
//! `cs_net::transport` (the pinned `renet2`/`renet2_netcode` `=0.16.1`
//! stack): the codec round-trips and refusals, the handshake's admit and
//! named reject paths, and the gate's deduplication of replayed input. The
//! sessions, signatures and packets are the newly authored synthetic
//! fixtures, never original data. Loopback on one machine is
//! `network_local` per `AGENTS.md`; `network_real` verification is F54-D.

use std::net::{Ipv4Addr, SocketAddr};
use std::time::Duration;

use cs_net::bounds::{MAX_INPUT_FRAMES_PER_PACKET, MAX_MODS, MAX_PACKET_BYTES};
use cs_net::codec::{
    ClientPacket, CodecError, ServerPacket, decode_client_packet, decode_server_packet,
    encode_client_message, encode_client_packet, encode_server_packet,
};
use cs_net::compat::{HandshakeReject, PROTOCOL_VERSION, SessionParameters};
use cs_net::fixture::{
    SYNTHETIC_CONTENT_SHA256, SYNTHETIC_RULES_SHA256, SYNTHETIC_SESSION, synthetic_hello,
    synthetic_input_message, synthetic_parameters, synthetic_peer_joined,
};
use cs_net::message::{
    ClientMessage, ClientPayload, DisconnectReason, MessageHeader, ServerMessage, ServerPayload,
    SnapshotFrame,
};
use cs_net::transport::{
    CHANNEL_SEQUENCED, ClientEvent, ClientTransport, DropReason, HostEvent, HostTransport,
    SendOutcome, TransportError,
};
use cs_net::validation::{Admission, SessionViolation, synthetic_fire_message};
use cs_types::Tick;
use cs_types::evidence::ContentHash;
use cs_types::net::{ActorId, PeerId, SessionAllocator, SessionId};

/// The elapsed time fed to each transport update. Loopback delivery needs no
/// real sleep; the updates only need to run often enough to exchange the
/// netcode handshake's packets.
const STEP: Duration = Duration::from_millis(16);

/// How many pump rounds an end-to-end expectation gets before it fails.
/// Each round is a client update plus a host update.
const MAX_ROUNDS: usize = 2_000;

/// A live loopback pair: one bound host and one connecting client, with the
/// events each side observed so far.
struct Link {
    host: HostTransport,
    client: ClientTransport,
    host_events: Vec<HostEvent>,
    client_events: Vec<ClientEvent>,
}

impl Link {
    /// Binds a host for `session`/`params` on loopback and connects a client
    /// offering `hello`.
    fn new(
        session: SessionId,
        params: SessionParameters,
        hello: cs_net::compat::ClientHello,
        client_id: u64,
    ) -> Self {
        let bind = SocketAddr::from((Ipv4Addr::LOCALHOST, 0));
        let host = HostTransport::bind(session, params, bind, Duration::ZERO)
            .expect("the host socket binds");
        let addr = host.local_addr().expect("the bound host has an address");
        let client = ClientTransport::connect(hello, addr, client_id, Duration::ZERO)
            .expect("the client socket binds");
        Self {
            host,
            client,
            host_events: Vec::new(),
            client_events: Vec::new(),
        }
    }

    /// One exchange round: the client updates (emitting its packets), then
    /// the host updates (receiving and answering).
    fn round(&mut self) {
        self.client_events.extend(self.client.update(STEP));
        self.host_events.extend(self.host.update(STEP));
    }

    /// Pumps until `wanted` matches a client event, returning all events.
    fn client_until(&mut self, wanted: impl Fn(&ClientEvent) -> bool) -> &[ClientEvent] {
        for _ in 0..MAX_ROUNDS {
            if self.client_events.iter().any(&wanted) {
                return &self.client_events;
            }
            self.round();
        }
        panic!(
            "no matching client event in {} rounds; saw {:?}",
            MAX_ROUNDS, self.client_events
        );
    }

    /// Pumps until `wanted` matches a host event, returning all events.
    fn host_until(&mut self, wanted: impl Fn(&HostEvent) -> bool) -> &[HostEvent] {
        for _ in 0..MAX_ROUNDS {
            if self.host_events.iter().any(&wanted) {
                return &self.host_events;
            }
            self.round();
        }
        panic!(
            "no matching host event in {} rounds; saw {:?}",
            MAX_ROUNDS, self.host_events
        );
    }

    /// Connects and completes the handshake, returning the grant.
    fn admitted(
        session: SessionId,
        params: SessionParameters,
        client_id: u64,
    ) -> (Self, cs_net::compat::SessionGrant) {
        let mut link = Self::new(session, params, synthetic_hello(), client_id);
        link.client_until(|event| {
            matches!(
                event,
                ClientEvent::Granted { .. } | ClientEvent::Rejected { .. }
            )
        });
        let grant = match link.client_events.iter().find_map(|event| match event {
            ClientEvent::Granted { grant } => Some(*grant),
            _ => None,
        }) {
            Some(grant) => grant,
            None => panic!("handshake did not grant: {:?}", link.client_events),
        };
        link.host_until(|event| matches!(event, HostEvent::PeerJoined { .. }));
        (link, grant)
    }
}

fn peer_packet_events(events: &[HostEvent]) -> Vec<&HostEvent> {
    events
        .iter()
        .filter(|event| matches!(event, HostEvent::PeerPacket { .. }))
        .collect()
}

#[test]
fn accept_f54_b_transport_crates_are_pinned_to_exact_versions() {
    let manifest_dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR"));
    let manifest = std::fs::read_to_string(manifest_dir.join("Cargo.toml"))
        .expect("cs_net Cargo.toml is readable");
    for package in ["renet2", "renet2_netcode", "renetcode2"] {
        let pinned = format!("{package} = {{ version = \"=0.16.1\"");
        assert!(
            manifest.contains(&pinned),
            "{package} must be pinned with an exact =0.16.1 requirement"
        );
    }
    let lock = std::fs::read_to_string(manifest_dir.join("../../Cargo.lock"))
        .expect("workspace Cargo.lock is readable");
    for package in ["renet2", "renet2_netcode", "renetcode2"] {
        let resolved = format!("name = \"{package}\"\nversion = \"0.16.1\"");
        assert!(
            lock.contains(&resolved),
            "Cargo.lock must resolve {package} to 0.16.1"
        );
    }
}

#[test]
fn accept_f54_b_handshake_packets_round_trip_the_codec() {
    let hello = ClientPacket::Hello(synthetic_hello());
    let bytes = encode_client_packet(&hello).expect("a hello encodes");
    assert!(bytes.len() <= MAX_PACKET_BYTES);
    assert_eq!(decode_client_packet(&bytes), Ok(hello));

    let grant = cs_net::compat::SessionGrant {
        session: SYNTHETIC_SESSION,
        peer: PeerId::new(3).expect("nonzero peer"),
    };
    for reply in [
        cs_net::compat::HelloReply::Welcome(grant),
        cs_net::compat::HelloReply::Rejected(HandshakeReject::ContentMismatch {
            expected: SYNTHETIC_CONTENT_SHA256,
            offered: ContentHash::from_bytes([0xAA; 32]),
        }),
        cs_net::compat::HelloReply::Rejected(HandshakeReject::UnsupportedProtocol {
            offered: cs_net::compat::ProtocolVersion::new(9).expect("nonzero version"),
            supported: PROTOCOL_VERSION,
        }),
    ] {
        let packet = ServerPacket::Reply(reply);
        let bytes = encode_server_packet(&packet).expect("a reply encodes");
        assert_eq!(decode_server_packet(&bytes), Ok(packet));
    }
}

#[test]
fn accept_f54_b_session_packets_round_trip_the_codec() {
    for message in [
        synthetic_input_message(7),
        synthetic_fire_message(SYNTHETIC_SESSION, Tick(42), 3),
        ClientMessage {
            header: MessageHeader {
                session: SYNTHETIC_SESSION,
                sequence: 4,
            },
            payload: ClientPayload::Leave,
        },
    ] {
        let bytes = encode_client_message(&message).expect("a client packet encodes");
        assert!(bytes.len() <= MAX_PACKET_BYTES);
        assert_eq!(
            decode_client_packet(&bytes),
            Ok(ClientPacket::Message(message))
        );
    }

    for payload in [
        ServerPayload::InputAck { through: 7 },
        ServerPayload::Snapshot(SnapshotFrame {
            tick: Tick(9),
            payload: vec![0xAB; 64],
        }),
        ServerPayload::Event(synthetic_peer_joined(
            PeerId::new(2).expect("nonzero peer"),
            Tick(5),
            1,
        )),
        ServerPayload::Disconnect {
            reason: DisconnectReason::SessionEnded,
        },
    ] {
        let message = ServerMessage {
            header: MessageHeader {
                session: SYNTHETIC_SESSION,
                sequence: 11,
            },
            payload,
        };
        let bytes =
            cs_net::codec::encode_server_message(&message).expect("a server packet encodes");
        assert!(bytes.len() <= MAX_PACKET_BYTES);
        assert_eq!(
            decode_server_packet(&bytes),
            Ok(ServerPacket::Message(message))
        );
    }
}

#[test]
fn accept_f54_b_oversized_buffers_and_counts_are_refused() {
    // A buffer past the packet cap never reaches the grammar.
    let oversized = vec![0u8; MAX_PACKET_BYTES + 1];
    assert_eq!(
        decode_client_packet(&oversized),
        Err(CodecError::TooLarge {
            field: "client_packet",
            max: MAX_PACKET_BYTES,
            len: MAX_PACKET_BYTES + 1,
        })
    );
    assert_eq!(
        decode_server_packet(&oversized),
        Err(CodecError::TooLarge {
            field: "server_packet",
            max: MAX_PACKET_BYTES,
            len: MAX_PACKET_BYTES + 1,
        })
    );

    // A count field past its cap is refused before the entries are read:
    // tag(1) + session(8) + sequence(4) + payload tag(1) + frame count(2).
    let mut batch = vec![2u8];
    batch.extend_from_slice(&1u64.to_le_bytes());
    batch.extend_from_slice(&0u32.to_le_bytes());
    batch.push(0); // ClientPayload::Input
    batch.extend_from_slice(&((MAX_INPUT_FRAMES_PER_PACKET + 1) as u16).to_le_bytes());
    assert_eq!(
        decode_client_packet(&batch),
        Err(CodecError::TooMany {
            field: "input.frames",
            max: MAX_INPUT_FRAMES_PER_PACKET,
            len: MAX_INPUT_FRAMES_PER_PACKET + 1,
        })
    );

    // The hello's mod list is capped the same way.
    let mut hello = vec![0u8];
    hello.extend_from_slice(&1u16.to_le_bytes());
    hello.extend_from_slice(&[0x52; 32]);
    hello.extend_from_slice(&[0xC5; 32]);
    hello.extend_from_slice(&((MAX_MODS + 1) as u16).to_le_bytes());
    assert_eq!(
        decode_client_packet(&hello),
        Err(CodecError::TooMany {
            field: "compat.mods",
            max: MAX_MODS,
            len: MAX_MODS + 1,
        })
    );
}

#[test]
fn accept_f54_b_truncated_and_trailing_buffers_are_refused() {
    let bytes =
        encode_client_packet(&ClientPacket::Hello(synthetic_hello())).expect("a hello encodes");
    for cut in [0, 1, bytes.len() / 2, bytes.len() - 1] {
        let outcome = decode_client_packet(&bytes[..cut]);
        assert!(
            matches!(outcome, Err(CodecError::Truncated { .. })),
            "cut {cut} must truncate, got {outcome:?}"
        );
    }
    let mut trailing = bytes.clone();
    trailing.push(0xFF);
    assert_eq!(
        decode_client_packet(&trailing),
        Err(CodecError::Trailing { len: 1 })
    );
    assert_eq!(
        decode_client_packet(&[]),
        Err(CodecError::Truncated {
            field: "packet.tag",
            needed: 1,
            remaining: 0,
        })
    );
}

#[test]
fn accept_f54_b_invalid_ids_and_tags_are_refused() {
    // A zero session id can never alias a live epoch.
    let mut packet = vec![2u8];
    packet.extend_from_slice(&0u64.to_le_bytes());
    packet.extend_from_slice(&0u32.to_le_bytes());
    packet.push(1); // ClientPayload::Leave
    assert_eq!(
        decode_client_packet(&packet),
        Err(CodecError::ZeroId {
            field: "header.session"
        })
    );

    // A zero peer id in a grant can never alias a live peer.
    let mut reply = vec![1u8, 0];
    reply.extend_from_slice(&1u64.to_le_bytes());
    reply.extend_from_slice(&0u16.to_le_bytes());
    assert_eq!(
        decode_server_packet(&reply),
        Err(CodecError::ZeroId {
            field: "reply.peer"
        })
    );

    // A protocol version of zero is not a version at all.
    let mut hello = vec![0u8];
    hello.extend_from_slice(&0u16.to_le_bytes());
    assert_eq!(
        decode_client_packet(&hello),
        Err(CodecError::ZeroId {
            field: "hello.protocol"
        })
    );

    // Unknown tags fail closed, client- and server-bound.
    let mut bad_payload = vec![2u8];
    bad_payload.extend_from_slice(&1u64.to_le_bytes());
    bad_payload.extend_from_slice(&0u32.to_le_bytes());
    bad_payload.push(99);
    assert_eq!(
        decode_client_packet(&bad_payload),
        Err(CodecError::UnknownTag {
            field: "client.tag",
            tag: 99
        })
    );
    assert_eq!(
        decode_server_packet(&[7u8]),
        Err(CodecError::UnknownTag {
            field: "packet.tag",
            tag: 7
        })
    );

    // Text that is not a catalog id is refused.
    let mut bad_mod = vec![0u8];
    bad_mod.extend_from_slice(&1u16.to_le_bytes());
    bad_mod.extend_from_slice(&[0x52; 32]);
    bad_mod.extend_from_slice(&[0xC5; 32]);
    bad_mod.extend_from_slice(&1u16.to_le_bytes());
    bad_mod.extend_from_slice(&3u16.to_le_bytes());
    bad_mod.extend_from_slice(b"!!!");
    assert_eq!(
        decode_client_packet(&bad_mod),
        Err(CodecError::BadText {
            field: "compat.mods[]"
        })
    );
}

#[test]
fn accept_f54_b_wire_validation_still_runs_after_decode() {
    // A structurally decodable packet whose contents violate the wire rules —
    // a client-owned UI action on the wire — is refused by the message's own
    // validation, not by the grammar.
    let mut frame = cs_types::input::InputFrame::new(Tick(1));
    frame.push_edge(cs_types::input::Action::Ui(
        cs_types::input::UiAction::Confirm,
    ));
    let message = ClientMessage {
        header: MessageHeader {
            session: SYNTHETIC_SESSION,
            sequence: 0,
        },
        payload: ClientPayload::Input(cs_net::message::InputBatch {
            frames: vec![frame],
        }),
    };
    let mut bytes = vec![2u8];
    bytes.extend_from_slice(&SYNTHETIC_SESSION.get().to_le_bytes());
    bytes.extend_from_slice(&0u32.to_le_bytes());
    bytes.push(0);
    bytes.extend_from_slice(&1u16.to_le_bytes());
    bytes.extend_from_slice(&1u64.to_le_bytes());
    bytes.extend_from_slice(&0u16.to_le_bytes()); // no axes
    bytes.extend_from_slice(&1u16.to_le_bytes()); // one edge
    bytes.push(1); // Action::Ui
    bytes.push(0); // UiAction::Confirm, first in UiAction::ALL
    assert!(
        matches!(
            decode_client_packet(&bytes),
            Err(CodecError::Wire(
                cs_net::message::WireError::UiActionOnWire { .. }
            ))
        ),
        "a UI edge on the wire must fail message validation: {message:?}"
    );
}

#[test]
fn accept_f54_b_handshake_admits_and_returns_a_grant_over_udp() {
    let (mut link, grant) = Link::admitted(SYNTHETIC_SESSION, synthetic_parameters(), 42);
    assert_eq!(grant.session, SYNTHETIC_SESSION);
    assert_eq!(grant.peer, PeerId::new(1).expect("the first peer id is 1"));
    assert_eq!(link.client.grant(), Some(grant));
    assert!(
        link.host_events
            .iter()
            .any(|event| matches!(event, HostEvent::PeerJoined { peer } if *peer == grant.peer)),
        "the host saw the join: {:?}",
        link.host_events
    );

    // A second client on the same host takes the next peer id; the grant is
    // the client's own.
    let addr = link.host.local_addr().expect("the host has an address");
    let mut second = ClientTransport::connect(synthetic_hello(), addr, 43, Duration::ZERO)
        .expect("the second client socket binds");
    let mut second_events = Vec::new();
    for _ in 0..MAX_ROUNDS {
        if second_events
            .iter()
            .any(|event| matches!(event, ClientEvent::Granted { .. }))
        {
            break;
        }
        second_events.extend(second.update(STEP));
        link.host_events.extend(link.host.update(STEP));
    }
    let second_grant = second.grant().expect("the second client is in");
    assert_eq!(second_grant.peer, PeerId::new(2).expect("a nonzero peer"));
    assert!(
        link.host_events.iter().any(
            |event| matches!(event, HostEvent::PeerJoined { peer } if *peer == second_grant.peer)
        ),
        "the host saw the second join: {:?}",
        link.host_events
    );
}

#[test]
fn accept_f54_b_rejection_arrives_with_the_named_reason() {
    let mut hello = synthetic_hello();
    hello.compatibility.rules_sha256 = ContentHash::from_bytes([0x00; 32]);
    let mut link = Link::new(SYNTHETIC_SESSION, synthetic_parameters(), hello, 42);

    link.client_until(|event| matches!(event, ClientEvent::Rejected { .. }));
    let rejection = link.client.rejection().expect("the rejection was stored");
    assert_eq!(
        *rejection,
        HandshakeReject::RulesMismatch {
            expected: SYNTHETIC_RULES_SHA256,
            offered: ContentHash::from_bytes([0x00; 32]),
        }
    );
    assert!(
        link.host_events
            .iter()
            .any(|event| matches!(event, HostEvent::PeerRejected { .. })),
        "the host saw the rejection: {:?}",
        link.host_events
    );
    assert_eq!(link.client.grant(), None);

    // The rejected client never becomes a peer: session traffic it sends
    // drops before reaching the gate.
    let bytes = encode_client_message(&synthetic_fire_message(SYNTHETIC_SESSION, Tick(1), 0))
        .expect("a fire packet encodes");
    link.client.send_encoded(CHANNEL_SEQUENCED, &bytes);
    link.host_until(|event| {
        matches!(
            event,
            HostEvent::PacketDropped {
                reason: DropReason::NoPeer,
                ..
            }
        )
    });
    assert!(
        peer_packet_events(&link.host_events).is_empty(),
        "a rejected client produced a peer packet: {:?}",
        link.host_events
    );
}

#[test]
fn accept_f54_b_session_traffic_before_a_grant_cannot_be_sent() {
    let mut link = Link::new(
        SYNTHETIC_SESSION,
        synthetic_parameters(),
        synthetic_hello(),
        42,
    );
    let outcome = link.client.send(synthetic_input_message(0));
    assert!(
        matches!(outcome, Err(TransportError::NotInSession)),
        "send before the grant must refuse, got {outcome:?}"
    );
}

#[test]
fn accept_f54_b_duplicate_input_cannot_duplicate_fire_over_udp() {
    let (mut link, grant) = Link::admitted(SYNTHETIC_SESSION, synthetic_parameters(), 42);
    let actor = ActorId {
        session: SYNTHETIC_SESSION,
        serial: 1,
    };
    link.host
        .gate_mut()
        .ownership_mut()
        .bind(grant.peer, actor)
        .expect("the host binds the client's aircraft");

    // One encoded fire packet, sent twice: the exact replay the acceptance
    // scenario describes.
    let bytes = encode_client_message(&synthetic_fire_message(SYNTHETIC_SESSION, Tick(20), 0))
        .expect("a fire packet encodes");
    link.client.send_encoded(CHANNEL_SEQUENCED, &bytes);
    link.client.send_encoded(CHANNEL_SEQUENCED, &bytes);
    link.host_until(|event| matches!(event, HostEvent::PeerPacket { .. }));
    // The duplicate may arrive in the same update; pump until a second
    // PeerPacket shows or enough rounds prove it never will.
    for _ in 0..50 {
        link.round();
        if peer_packet_events(&link.host_events).len() >= 2 {
            break;
        }
    }
    let packets = peer_packet_events(&link.host_events);
    assert_eq!(packets.len(), 2, "both deliveries must reach the gate");
    match packets[0] {
        HostEvent::PeerPacket {
            peer,
            admission,
            fires,
        } => {
            assert_eq!(*peer, grant.peer);
            assert_eq!(*admission, Admission::Accepted { sequence: 0 });
            assert_eq!(fires.len(), 1, "the first delivery fires once");
            assert_eq!(fires[0].actor, actor);
            assert_eq!(fires[0].sequence, 0);
        }
        other => panic!("expected a peer packet, saw {other:?}"),
    }
    match packets[1] {
        HostEvent::PeerPacket {
            peer,
            admission,
            fires,
        } => {
            assert_eq!(*peer, grant.peer);
            assert!(
                matches!(
                    admission,
                    Admission::Refused(SessionViolation::ReplayedSequence { .. })
                ),
                "the replay must be refused as a replay: {admission:?}"
            );
            assert!(
                fires.is_empty(),
                "a replayed packet must authorize no fire at all"
            );
        }
        other => panic!("expected a peer packet, saw {other:?}"),
    }
}

#[test]
fn accept_f54_b_out_of_order_input_cannot_duplicate_fire_over_udp() {
    let (mut link, grant) = Link::admitted(SYNTHETIC_SESSION, synthetic_parameters(), 42);
    let actor = ActorId {
        session: SYNTHETIC_SESSION,
        serial: 1,
    };
    link.host
        .gate_mut()
        .ownership_mut()
        .bind(grant.peer, actor)
        .expect("the host binds the client's aircraft");

    // A later packet lands first, then the earlier one arrives: the delayed
    // delivery is out of order and must not fire.
    let late = encode_client_message(&synthetic_fire_message(SYNTHETIC_SESSION, Tick(21), 9))
        .expect("a fire packet encodes");
    let early = encode_client_message(&synthetic_fire_message(SYNTHETIC_SESSION, Tick(20), 8))
        .expect("a fire packet encodes");
    link.client.send_encoded(CHANNEL_SEQUENCED, &late);
    link.client.send_encoded(CHANNEL_SEQUENCED, &early);
    for _ in 0..200 {
        link.round();
        if peer_packet_events(&link.host_events).len() >= 2 {
            break;
        }
    }
    let packets = peer_packet_events(&link.host_events);
    assert_eq!(packets.len(), 2, "both deliveries must reach the gate");
    match packets[0] {
        HostEvent::PeerPacket {
            peer,
            admission,
            fires,
        } => {
            assert_eq!(*peer, grant.peer);
            assert_eq!(*admission, Admission::Accepted { sequence: 9 });
            assert_eq!(fires.len(), 1, "the in-order delivery fires once");
        }
        other => panic!("expected a peer packet, saw {other:?}"),
    }
    match packets[1] {
        HostEvent::PeerPacket {
            peer,
            admission,
            fires,
        } => {
            assert_eq!(*peer, grant.peer);
            assert!(
                matches!(
                    admission,
                    Admission::Refused(SessionViolation::ReplayedSequence { .. })
                ),
                "the out-of-order delivery must be refused: {admission:?}"
            );
            assert!(
                fires.is_empty(),
                "an out-of-order packet must authorize no fire at all"
            );
        }
        other => panic!("expected a peer packet, saw {other:?}"),
    }
}

#[test]
fn accept_f54_b_server_packets_reach_the_granted_client() {
    let (mut link, grant) = Link::admitted(SYNTHETIC_SESSION, synthetic_parameters(), 42);
    let event = ServerMessage {
        header: MessageHeader {
            session: SessionId::new(999).expect("a nonzero session"),
            sequence: 999,
        },
        payload: ServerPayload::Event(synthetic_peer_joined(grant.peer, Tick(3), 0)),
    };
    let outcome = link
        .host
        .send(grant.peer, event)
        .expect("the event encodes");
    assert_eq!(outcome, SendOutcome::Sent);
    link.client_until(|client_event| matches!(client_event, ClientEvent::Server(_)));
    let message = link
        .client_events
        .iter()
        .find_map(|client_event| match client_event {
            ClientEvent::Server(message) => Some(message),
            _ => None,
        })
        .expect("a server packet arrived");
    // The transport stamped the host's epoch and its own sequence, not the
    // values the caller wrote.
    assert_eq!(message.header.session, SYNTHETIC_SESSION);
    assert_eq!(message.header.sequence, 0);
    assert_eq!(
        message.payload,
        ServerPayload::Event(synthetic_peer_joined(grant.peer, Tick(3), 0))
    );
}

#[test]
fn accept_f54_b_disconnect_event_arrives_once() {
    let (mut link, grant) = Link::admitted(SYNTHETIC_SESSION, synthetic_parameters(), 42);
    link.client.disconnect();
    for _ in 0..8 {
        link.round();
    }
    let disconnects = link
        .client_events
        .iter()
        .filter(|event| matches!(event, ClientEvent::Disconnected { .. }))
        .count();
    assert_eq!(
        disconnects, 1,
        "the disconnect reason is persistent inside renet; the event must still fire once: {:?}",
        link.client_events
    );
    link.host_until(
        |event| matches!(event, HostEvent::PeerDeparted { peer } if *peer == grant.peer),
    );
}

#[test]
fn accept_f54_b_session_allocator_mints_live_epochs() {
    let mut allocator = SessionAllocator::new();
    let first = allocator.allocate().expect("the first session allocates");
    let second = allocator.allocate().expect("the second session allocates");
    assert_eq!(first, SessionId::new(1).expect("session 1"));
    assert!(second > first, "session epochs are monotonic");

    // A minted epoch is a live session the transport binds and grants.
    let (_link, grant) = Link::admitted(second, synthetic_parameters(), 42);
    assert_eq!(grant.session, second);
}

#[test]
fn accept_f54_b_a_peer_leave_departs_the_session() {
    let (mut link, grant) = Link::admitted(SYNTHETIC_SESSION, synthetic_parameters(), 42);
    link.client
        .send(ClientMessage {
            header: MessageHeader {
                session: SYNTHETIC_SESSION,
                sequence: 0,
            },
            payload: ClientPayload::Leave,
        })
        .expect("the leave encodes");
    link.host_until(
        |event| matches!(event, HostEvent::PeerDeparted { peer } if *peer == grant.peer),
    );
    assert!(!link.host.gate().is_member(grant.peer));
}
