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
use std::sync::{Mutex, MutexGuard};
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
    CHANNEL_SEQUENCED, ClientEvent, ClientTransport, ConnectWindow, DEFAULT_CONNECT_WINDOW,
    DropReason, HostEvent, HostTransport, SendOutcome, TransportError,
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
///
/// Derived from [`LOOPBACK_WINDOW`] and [`STEP`] together, so neither can drift
/// out from under the other: the budget is the connection layer's own silence
/// window expressed in [`STEP`]s, plus a margin. A bare constant cannot hold
/// that relation — the `2_000` this replaced was 32 seconds of accumulated pump
/// time against a 15-second window, more than twice the patience it was trying
/// to outlast, so the test could be waiting on something the connection layer
/// had already given up on. It is still a bound on rounds, not a sleep: no test
/// here waits on the wall clock.
const MAX_ROUNDS: usize =
    (LOOPBACK_WINDOW.const_seconds() as usize * 1_000 / STEP.as_millis() as usize) + 64;

/// The connection-layer window these acceptance tests ask for.
///
/// The pinned layer's own default is fifteen seconds
/// ([`DEFAULT_CONNECT_WINDOW`]), and it accumulates that window entirely from
/// the `elapsed` a caller hands to `update`. A test that pumps at [`STEP`]
/// therefore gives itself fifteen seconds of *pump* time in about four
/// milliseconds of wall clock, and a single dropped loopback datagram on a
/// machine that is oversubscribed cannot be retransmitted inside that window:
/// the client disconnects, and the pinned state machine has no way back. See
/// `docs/findings/2026-10-04-f54-x2-loopback-pump-and-socket-determinism.md`
/// for the measurement.
///
/// This is a fixture parameter only. A shipped session runs with the default
/// window, which no code path here changes; widening it is also what
/// `accept_f54_b_the_connect_window_is_a_fixture_parameter_over_the_default`
/// pins.
const LOOPBACK_WINDOW: ConnectWindow = ConnectWindow::new(120);

/// Serializes the tests in this file that open a real loopback socket.
///
/// Measured on an 11-core machine deliberately oversubscribed more than
/// twofold: with many short-lived socket pairs churning at once, 380 of 1500
/// loopback pairs lost one or two of 59 datagrams each, while a single pair at
/// the same load settled 600 of 600 (same finding, sections 4 and 6). The loss
/// is in the machine's loopback UDP path, not in anything a test asserts, so
/// this file keeps at most one [`Link`] — one bound host and its client — alive
/// at a time instead of racing for the loopback path. Each test here is a few
/// milliseconds of socket work, so serializing costs nothing measurable and
/// takes the machine's load out of the result. The one place that opens a
/// further socket on purpose is
/// `accept_f54_b_handshake_admits_and_returns_a_grant_over_udp`, which needs a
/// second peer id on the host it already holds.
///
/// `cargo test` runs this binary concurrently with
/// `accept_f54_c_lifecycle`, so that file's own lock cannot serialize against
/// this one; each file holds its own, and together they cover this crate's
/// loopback acceptance tests.
static LOOPBACK: Mutex<()> = Mutex::new(());

/// Takes the loopback socket lock. Held for as long as the sockets it guards.
fn loopback() -> MutexGuard<'static, ()> {
    LOOPBACK
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
}

/// How many silent rounds a client is given before its verdict is read.
///
/// Comfortably past [`DEFAULT_CONNECT_WINDOW`] expressed in [`STEP`]s, so a
/// client that asked for the default window must be gone by then, and far
/// short of [`LOOPBACK_WINDOW`]'s, so a client that asked for this file's
/// window must still be there. Derived the same way [`MAX_ROUNDS`] is, with the
/// same margin, so neither number is a bare constant.
const SILENT_ROUNDS: usize =
    (DEFAULT_CONNECT_WINDOW.const_seconds() as usize * 1_000 / STEP.as_millis() as usize) + 64;

/// A live loopback pair: one bound host and one connecting client, with the
/// events each side observed so far.
struct Link {
    host: HostTransport,
    client: ClientTransport,
    host_events: Vec<HostEvent>,
    client_events: Vec<ClientEvent>,
    /// Held for as long as the two sockets above are open, so this file never
    /// has two `Link`s alive at once. See [`loopback`].
    ///
    /// A further socket on a host this one already owns is still possible by
    /// design — see
    /// `accept_f54_b_handshake_admits_and_returns_a_grant_over_udp` — so what
    /// this field guarantees is "one `Link` at a time", not "one socket at a
    /// time".
    _loopback: MutexGuard<'static, ()>,
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
        let loopback = loopback();
        let bind = SocketAddr::from((Ipv4Addr::LOCALHOST, 0));
        let host = HostTransport::bind(session, params, bind, Duration::ZERO)
            .expect("the host socket binds");
        let addr = host.local_addr().expect("the bound host has an address");
        let client = ClientTransport::connect_with_window(
            hello,
            addr,
            client_id,
            Duration::ZERO,
            LOOPBACK_WINDOW,
        )
        .expect("the client socket binds");
        Self {
            host,
            client,
            host_events: Vec::new(),
            client_events: Vec::new(),
            _loopback: loopback,
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
fn accept_f54_b_the_connect_window_is_a_fixture_parameter_over_the_default() {
    // This test holds the loopback lock itself and binds its own sockets
    // directly rather than going through `Link`, which takes that same lock: a
    // test that did both would wait on a lock it already holds.
    let _loopback = loopback();

    // The pinned layer's own unsecure token carries a fifteen-second window;
    // that is what `DEFAULT_CONNECT_WINDOW` is and what every shipped session
    // runs with, so the fixture parameter below only ever *widens* what a
    // caller already had.
    assert_eq!(
        DEFAULT_CONNECT_WINDOW.const_seconds(),
        15,
        "the default is the pinned stack's own value, not a number this crate chose"
    );
    assert!(
        LOOPBACK_WINDOW.const_seconds() > DEFAULT_CONNECT_WINDOW.const_seconds(),
        "the fixture window must widen the default, never narrow it"
    );

    // The round budget is the window expressed in STEPs plus a margin. The
    // derivation is spelled out here rather than repeated from the expression
    // `MAX_ROUNDS` is defined by, so this fails if the budget is ever replaced
    // by a bare constant again — which is what it was before this file asked
    // for a window of its own.
    assert_eq!(
        MAX_ROUNDS,
        LOOPBACK_WINDOW.const_seconds() as usize * 1_000 / STEP.as_millis() as usize + 64,
        "the round budget is the connection layer's window in STEPs, plus a margin"
    );
    let window_ms = LOOPBACK_WINDOW.const_seconds() as usize * 1_000;
    let budget_ms = MAX_ROUNDS * STEP.as_millis() as usize;
    assert!(
        budget_ms >= window_ms,
        "the budget must outlast the window it waits through ({budget_ms} ms against \
         {window_ms} ms), or a handshake still inside the connection layer's \
         patience can be reported as a failure"
    );
    assert!(
        budget_ms < window_ms * 2,
        "the budget must track the window closely, not run far past it \
         ({budget_ms} ms against {window_ms} ms): a budget that outlasts the \
         window by much more than its own margin is one that can wait on a \
         connection the layer has already given up on"
    );

    // The window is the connection layer's own, measured in the `elapsed` a
    // caller pumps with: a client that asked for the default window is dropped
    // after about that much silence, while a client that asked for this file's
    // window is still there. Nothing in `cs_net` decides this — the pinned layer
    // reads it straight out of the connect token it decodes, so the two halves
    // of the fix are observable from the production API.
    for (window, still_connected) in [(DEFAULT_CONNECT_WINDOW, false), (LOOPBACK_WINDOW, true)] {
        let mut host = HostTransport::bind(
            SYNTHETIC_SESSION,
            synthetic_parameters(),
            SocketAddr::from((Ipv4Addr::LOCALHOST, 0)),
            Duration::ZERO,
        )
        .expect("the host socket binds");
        let addr = host.local_addr().expect("the bound host has an address");
        let mut speaker = ClientTransport::connect_with_window(
            synthetic_hello(),
            addr,
            0xB0,
            Duration::ZERO,
            window,
        )
        .expect("the client socket binds");

        // Whichever window it asked for, the handshake itself is unchanged: a
        // widened client gets the same grant a default client gets.
        let mut granted = None;
        for _ in 0..MAX_ROUNDS {
            speaker.update(STEP);
            host.update(STEP);
            if let Some(grant) = speaker.grant() {
                granted = Some(grant);
                break;
            }
        }
        let grant = granted.unwrap_or_else(|| panic!("the {window:?} handshake completed"));
        assert_eq!(grant.session, SYNTHETIC_SESSION);

        // The host goes quiet: it is never pumped again, so it stops sending
        // and the connection layer's own silence window is what runs.
        for _ in 0..SILENT_ROUNDS {
            speaker.update(STEP);
        }
        assert_eq!(
            speaker.is_connected(),
            still_connected,
            "after {SILENT_ROUNDS} silent rounds ({window:?} window) the connection \
             layer's verdict is the window's"
        );
    }
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
    // the client's own. It lives inside the first link's loopback guard, so
    // this is the one place the file has three sockets open at once — the
    // first link's host, its client and this one — and the scenario needs the
    // second peer, so it is not serialized further. What it does do is ask for
    // the same window as every other client here rather than the default one.
    let addr = link.host.local_addr().expect("the host has an address");
    let mut second = ClientTransport::connect_with_window(
        synthetic_hello(),
        addr,
        43,
        Duration::ZERO,
        LOOPBACK_WINDOW,
    )
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
            input,
            fires,
        } => {
            assert_eq!(*peer, grant.peer);
            assert_eq!(*admission, Admission::Accepted { sequence: 0 });
            assert_eq!(fires.len(), 1, "the first delivery fires once");
            assert_eq!(fires[0].actor, actor);
            assert_eq!(fires[0].sequence, 0);
            let admitted = input
                .as_ref()
                .expect("the admitted frames reach the consumer");
            assert_eq!(admitted.sequence, 0);
            assert_eq!(admitted.batch.frames.len(), 1, "one frame was sent");
        }
        other => panic!("expected a peer packet, saw {other:?}"),
    }
    match packets[1] {
        HostEvent::PeerPacket {
            peer,
            admission,
            input,
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
            assert!(
                input.is_none(),
                "a replayed packet must hand the consumer no input at all"
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
            input,
            fires,
        } => {
            assert_eq!(*peer, grant.peer);
            assert_eq!(*admission, Admission::Accepted { sequence: 9 });
            assert_eq!(fires.len(), 1, "the in-order delivery fires once");
            let admitted = input
                .as_ref()
                .expect("the admitted frames reach the consumer");
            assert_eq!(admitted.sequence, 9);
            assert_eq!(admitted.batch.frames.len(), 1, "one frame was sent");
        }
        other => panic!("expected a peer packet, saw {other:?}"),
    }
    match packets[1] {
        HostEvent::PeerPacket {
            peer,
            admission,
            input,
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
            assert!(
                input.is_none(),
                "an out-of-order packet must hand the consumer no input at all"
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
    link.client
        .disconnect()
        .expect("the connection is live, so the hang-up reports nothing");
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
