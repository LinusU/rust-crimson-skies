//! F55-BU1-WIRE acceptance: the lobby packet envelope on the F54 session
//! wire.
//!
//! A [`LobbyPacket`] encoded by the production codec crosses a real loopback
//! session (`ClientSession::send_lobby` → `ClientPayload::Lobby` → the session
//! gate → `ServerSession` → `Lobby::receive_bytes`), the lobby's events come
//! back as reliable `EventBody::Lobby` events the client decodes, and a refusal
//! — a launch for an old rules revision — is reported to the sender, not only
//! to the host process.

use std::collections::BTreeSet;
use std::net::{Ipv4Addr, SocketAddr};
use std::sync::MutexGuard;
use std::time::Duration;

use cs_net::codec::{
    ClientPacket, ServerPacket, decode_client_packet, decode_server_packet, encode_client_message,
    encode_server_message,
};
use cs_net::fixture::{SYNTHETIC_SESSION, synthetic_hello, synthetic_parameters};
use cs_net::lifecycle::{ClientFault, ClientNotice, ClientSession, ServerNotice, ServerSession};
use cs_net::lobby::*;
use cs_net::message::{
    ClientMessage, ClientPayload, EventBody, LobbyRefusal, MAX_LOBBY_RECORD_BYTES, MessageHeader,
    ReliableEvent, ServerMessage, ServerPayload, WireError,
};
use cs_net::transport::ConnectWindow;
use cs_types::Tick;
use cs_types::content::{ContentId, ContentKind};
use cs_types::net::{EventId, PeerId};

const STEP: Duration = Duration::from_millis(16);
const WINDOW: ConnectWindow = ConnectWindow::new(120);
const ROUNDS: usize = 4_000;
const ATTEMPTS: usize = 24;

struct AcceptAll;

impl LoadoutValidator for AcceptAll {
    fn validate(&self, _: &Loadout) -> Result<(), &'static str> {
        Ok(())
    }
}

fn id(kind: ContentKind, key: &str) -> ContentId {
    ContentId::from_source(kind, key).unwrap()
}

fn callsign(text: &str) -> Callsign {
    Callsign::new(text).unwrap()
}

fn open_lobby() -> Lobby {
    Lobby::open(
        SYNTHETIC_SESSION,
        synthetic_parameters(),
        Admission {
            access: AccessPolicy::Open,
            max_members: 4,
            host_loss: HostLoss::EndSession,
        },
        LobbyRules {
            scenario: id(ContentKind::MultiplayerScenario, "synthetic_duel"),
            banned: BTreeSet::new(),
            team_mode: TeamMode::FreeForAll,
            late_join: LateJoin::Closed,
        },
        callsign("Host"),
    )
    .unwrap()
}

fn packet_id(sequence: u32) -> EventId {
    EventId {
        session: SYNTHETIC_SESSION,
        tick: Tick(3),
        producer: 7,
        sequence,
    }
}

fn chat(sequence: u32, text: &str) -> LobbyPacket {
    LobbyPacket::Command {
        id: packet_id(sequence),
        command: LobbyCommand::Peer(PeerRequest::Chat(ChatText::new(text).unwrap())),
    }
}

/// A live loopback pair with a lobby attached to the host session.
struct Wire {
    host: ServerSession,
    client: ClientSession,
    host_notices: Vec<ServerNotice>,
    client_notices: Vec<ClientNotice>,
    /// The client's session peer.
    peer: PeerId,
    _loopback: MutexGuard<'static, ()>,
}

impl Wire {
    /// Builds a joined pair, rebuilding when the loopback underneath dies
    /// before the handshake completes.
    fn joined() -> Self {
        for _ in 0..ATTEMPTS {
            if let Some(wire) = Self::try_joined() {
                return wire;
            }
            std::thread::sleep(Duration::from_millis(100));
        }
        panic!("the loopback never carried a handshake");
    }

    fn try_joined() -> Option<Self> {
        let guard = crate::support::loopback::lock();
        let bind = SocketAddr::from((Ipv4Addr::LOCALHOST, 0));
        let host = ServerSession::bind(
            SYNTHETIC_SESSION,
            synthetic_parameters(),
            bind,
            Duration::ZERO,
        )
        .expect("the host binds");
        let addr = host.local_addr().expect("the host has an address");
        let client = ClientSession::connect_with_window(
            synthetic_hello(),
            addr,
            0xC1,
            Duration::ZERO,
            WINDOW,
        )
        .expect("the client binds");
        let mut wire = Self {
            host,
            client,
            host_notices: Vec::new(),
            client_notices: Vec::new(),
            peer: PeerId::new(1).unwrap(),
            _loopback: guard,
        };
        for _ in 0..ROUNDS {
            if wire.client.grant().is_some()
                && wire
                    .host
                    .members()
                    .next()
                    .is_some_and(|peer| wire.peer == peer)
            {
                return Some(wire);
            }
            wire.round();
        }
        None
    }

    fn round(&mut self) {
        self.client_notices.extend(self.client.pump(STEP));
        self.host_notices.extend(self.host.pump(STEP));
    }

    /// Pumps until `done` holds; the wait is on the condition, not a sleep.
    fn until(&mut self, what: &str, done: impl Fn(&Self) -> bool) {
        for _ in 0..ROUNDS {
            if done(self) {
                return;
            }
            self.round();
        }
        panic!(
            "{what}: host {:?}; client {:?}",
            self.host_notices, self.client_notices
        );
    }

    fn pump(&mut self, rounds: usize) {
        for _ in 0..rounds {
            self.round();
        }
    }

    fn lobby_events(&self) -> Vec<(EventId, LobbyEvent)> {
        self.client_notices
            .iter()
            .filter_map(|notice| match notice {
                ClientNotice::Lobby { id, event } => Some((*id, event.clone())),
                _ => None,
            })
            .collect()
    }
}

#[test]
fn accept_f55_bu1_a_lobby_packet_crosses_the_session_wire_and_its_events_come_back() {
    let mut wire = Wire::joined();
    let mut lobby = open_lobby();
    let (member, joined) = lobby
        .admit(JoinRequest {
            hello: synthetic_hello(),
            callsign: callsign("Ace"),
            password: None,
        })
        .unwrap();
    wire.host
        .attach_lobby(lobby, Box::new(AcceptAll))
        .expect("the lobby is for this epoch");
    wire.host.bind_lobby_member(wire.peer, member).unwrap();
    wire.host.publish_lobby(&joined.events).unwrap();
    wire.until("the client never saw the join", |w| {
        w.lobby_events()
            .iter()
            .any(|(_, event)| *event == LobbyEvent::MemberJoined { peer: member })
    });

    let packet = chat(11, "hello hangar");
    wire.client.send_lobby(&packet).expect("the packet is sent");
    wire.until("the chat never came back", |w| {
        w.lobby_events()
            .iter()
            .any(|(_, event)| matches!(event, LobbyEvent::Chat { .. }))
    });

    // The host's lobby received exactly the packet that was encoded.
    assert!(
        wire.host_notices.iter().any(|notice| matches!(
            notice,
            ServerNotice::LobbyHandled { peer, outcome: PacketOutcome::Applied(_) }
                if *peer == wire.peer
        )),
        "{:?}",
        wire.host_notices
    );
    let chats: Vec<_> = wire
        .lobby_events()
        .into_iter()
        .filter_map(|(_, event)| match event {
            LobbyEvent::Chat { from, text } => Some((from, text)),
            _ => None,
        })
        .collect();
    assert_eq!(chats.len(), 1);
    assert_eq!(chats[0].0, member, "the host stamped the lobby member");
    assert_eq!(chats[0].1.as_str(), "hello hangar");
}

#[test]
fn accept_f55_bu1_a_redelivered_packet_is_not_applied_twice_on_the_wire() {
    let mut wire = Wire::joined();
    let mut lobby = open_lobby();
    let (member, _) = lobby
        .admit(JoinRequest {
            hello: synthetic_hello(),
            callsign: callsign("Ace"),
            password: None,
        })
        .unwrap();
    wire.host.attach_lobby(lobby, Box::new(AcceptAll)).unwrap();
    wire.host.bind_lobby_member(wire.peer, member).unwrap();

    // The same packet three times: each takes a fresh session sequence, so the
    // session gate admits all of them and only the lobby's EventId guard can
    // stop the repeat.
    let packet = chat(5, "once");
    // One per round: a burst past the per-tick share is rate limited, which is
    // a different guard from the one under test.
    for copy in 1..=3 {
        wire.client.send_lobby(&packet).unwrap();
        wire.until("the host never handled the copy", |w| {
            w.host_notices
                .iter()
                .filter(|notice| matches!(notice, ServerNotice::LobbyHandled { .. }))
                .count()
                >= copy
        });
    }
    wire.pump(8);

    let outcomes: Vec<_> = wire
        .host_notices
        .iter()
        .filter_map(|notice| match notice {
            ServerNotice::LobbyHandled { outcome, .. } => Some(outcome.clone()),
            _ => None,
        })
        .collect();
    assert!(matches!(outcomes[0], PacketOutcome::Applied(_)));
    assert_eq!(outcomes[1], PacketOutcome::Replayed);
    assert_eq!(outcomes[2], PacketOutcome::Replayed);
    let chats = wire
        .lobby_events()
        .iter()
        .filter(|(_, event)| matches!(event, LobbyEvent::Chat { .. }))
        .count();
    assert_eq!(chats, 1, "the chat line is broadcast once");
}

#[test]
fn accept_f55_bu1_a_launch_for_an_old_rules_revision_is_rejected_end_to_end() {
    let mut wire = Wire::joined();
    let mut lobby = open_lobby();
    let host_member = lobby.host();
    let mg = id(ContentKind::Gun, "synthetic_mg");
    let cannon = id(ContentKind::Gun, "synthetic_cannon");
    lobby
        .apply(
            host_member,
            LobbyCommand::Peer(PeerRequest::SetLoadout(Loadout {
                blueprint: id(ContentKind::Blueprint, "synthetic_zephyr"),
                components: vec![mg],
            })),
            &AcceptAll,
        )
        .unwrap();
    let first = lobby.revision();
    lobby
        .apply(
            host_member,
            LobbyCommand::Peer(PeerRequest::SetReady { revision: first }),
            &AcceptAll,
        )
        .unwrap();
    let old = LaunchRequest {
        revision: lobby.revision(),
        digest: lobby.digest(),
    };
    // The host's player is the session's one client.
    wire.host.attach_lobby(lobby, Box::new(AcceptAll)).unwrap();
    wire.host.bind_lobby_member(wire.peer, host_member).unwrap();

    // The host bans a component: the live revision moves past `old`.
    let ban = wire
        .host
        .lobby_mut()
        .unwrap()
        .apply(
            host_member,
            LobbyCommand::Host(HostAction::Ban(cannon)),
            &AcceptAll,
        )
        .unwrap();
    wire.host.publish_lobby(&ban.events).unwrap();
    assert_ne!(wire.host.lobby().unwrap().revision(), old.revision);

    let stale = LobbyPacket::Launch {
        id: packet_id(1),
        request: old,
    };
    wire.client.send_lobby(&stale).unwrap();
    wire.until("the client was never told", |w| {
        w.client_notices
            .iter()
            .any(|notice| matches!(notice, ClientNotice::LobbyRefused { .. }))
    });
    assert!(wire.client_notices.iter().any(|notice| matches!(
        notice,
        ClientNotice::LobbyRefused { packet, refusal: LobbyRefusal::StaleRevision }
            if *packet == packet_id(1)
    )));
    assert!(wire.host_notices.iter().any(|notice| matches!(
        notice,
        ServerNotice::LobbyRefused {
            reject: PacketReject::Launch(LaunchError::StaleRevision { .. }),
            ..
        }
    )));
    assert_eq!(wire.host.lobby().unwrap().phase(), Phase::Gathering);

    // The live revision under the same wire is accepted, so the refusal above
    // was about the revision and not the path.
    let live = LobbyPacket::Launch {
        id: packet_id(2),
        request: {
            let lobby = wire.host.lobby().unwrap();
            LaunchRequest {
                revision: lobby.revision(),
                digest: lobby.digest(),
            }
        },
    };
    wire.client.send_lobby(&live).unwrap();
    wire.until("the live launch was never handled", |w| {
        w.host_notices.iter().any(|notice| {
            matches!(
                notice,
                ServerNotice::LobbyHandled {
                    outcome: PacketOutcome::Launch(_),
                    ..
                }
            )
        })
    });
}

#[test]
fn accept_f55_bu1_a_peer_with_no_lobby_membership_is_refused_and_told() {
    let mut wire = Wire::joined();
    wire.host
        .attach_lobby(open_lobby(), Box::new(AcceptAll))
        .unwrap();
    // The session peer was never bound to a lobby member.
    wire.client.send_lobby(&chat(1, "hi")).unwrap();
    wire.until("the client was never told", |w| {
        w.client_notices.iter().any(|notice| {
            matches!(
                notice,
                ClientNotice::LobbyRefused {
                    refusal: LobbyRefusal::NotAMember,
                    ..
                }
            )
        })
    });
    assert!(wire.lobby_events().is_empty());
}

#[test]
fn accept_f55_bu1_the_attachment_refuses_a_foreign_epoch_and_a_second_lobby() {
    let mut wire = Wire::joined();
    let foreign = Lobby::open(
        cs_types::net::SessionId::new(99).unwrap(),
        synthetic_parameters(),
        Admission {
            access: AccessPolicy::Open,
            max_members: 2,
            host_loss: HostLoss::EndSession,
        },
        LobbyRules {
            scenario: id(ContentKind::MultiplayerScenario, "synthetic_duel"),
            banned: BTreeSet::new(),
            team_mode: TeamMode::FreeForAll,
            late_join: LateJoin::Closed,
        },
        callsign("Host"),
    )
    .unwrap();
    assert!(
        wire.host
            .attach_lobby(foreign, Box::new(AcceptAll))
            .is_err()
    );
    wire.host
        .attach_lobby(open_lobby(), Box::new(AcceptAll))
        .unwrap();
    assert!(
        wire.host
            .attach_lobby(open_lobby(), Box::new(AcceptAll))
            .is_err()
    );
    // A peer or member that is not there cannot be bound.
    let ghost = PeerId::new(9).unwrap();
    assert!(wire.host.bind_lobby_member(ghost, ghost).is_err());
    assert!(wire.host.bind_lobby_member(wire.peer, ghost).is_err());
}

#[test]
fn accept_f55_bu1_the_envelope_records_are_bounded_and_round_trip() {
    let session = SYNTHETIC_SESSION;
    let header = MessageHeader {
        session,
        sequence: 4,
    };
    // The client direction round-trips and is reliable.
    let record = chat(2, "round trip").encode().unwrap();
    let message = ClientMessage {
        header,
        payload: ClientPayload::Lobby(record.clone()),
    };
    assert_eq!(message.delivery(), cs_net::message::Delivery::Reliable);
    let bytes = encode_client_message(&message).unwrap();
    match decode_client_packet(&bytes).unwrap() {
        ClientPacket::Message(decoded) => assert_eq!(decoded, message),
        other => panic!("{other:?}"),
    }
    // An oversized lobby record is refused at the envelope.
    let oversized = ClientMessage {
        header,
        payload: ClientPayload::Lobby(vec![0; MAX_LOBBY_RECORD_BYTES + 1]),
    };
    assert!(matches!(
        oversized.validate(),
        Err(WireError::TooLarge { .. })
    ));
    assert!(encode_client_message(&oversized).is_err());

    // The server direction: an event and a refusal round-trip.
    for body in [
        EventBody::Lobby(LobbyEvent::LaunchCancelled.encode().unwrap()),
        EventBody::LobbyRefused {
            packet: packet_id(3),
            refusal: LobbyRefusal::StaleRevision,
        },
    ] {
        let message = ServerMessage {
            header,
            payload: ServerPayload::Event(ReliableEvent {
                id: packet_id(8),
                body,
            }),
        };
        assert_eq!(message.delivery(), cs_net::message::Delivery::Reliable);
        let bytes = encode_server_message(&message).unwrap();
        match decode_server_packet(&bytes).unwrap() {
            ServerPacket::Message(decoded) => assert_eq!(decoded, message),
            other => panic!("{other:?}"),
        }
    }
}

#[test]
fn accept_f55_bu1_every_lobby_event_round_trips_through_its_codec() {
    let peer = PeerId::new(3).unwrap();
    let events = [
        LobbyEvent::RulesChanged {
            revision: Revision::new(4).unwrap(),
            digest: open_lobby().digest(),
        },
        LobbyEvent::ReadyRevoked(ReadyRevocation {
            peer,
            reason: RevokeReason::ComponentBanned {
                component: id(ContentKind::Gun, "synthetic_cannon"),
            },
        }),
        LobbyEvent::ReadyRevoked(ReadyRevocation {
            peer,
            reason: RevokeReason::ScenarioChanged,
        }),
        LobbyEvent::ReadyRevoked(ReadyRevocation {
            peer,
            reason: RevokeReason::TeamsChanged,
        }),
        LobbyEvent::ReadyRevoked(ReadyRevocation {
            peer,
            reason: RevokeReason::SelfEdited,
        }),
        LobbyEvent::MemberJoined { peer },
        LobbyEvent::MemberLeft { peer },
        LobbyEvent::MemberReady { peer },
        LobbyEvent::Chat {
            from: peer,
            text: ChatText::new("héllo").unwrap(),
        },
        LobbyEvent::LaunchCancelled,
        LobbyEvent::PhaseChanged(Phase::Gathering),
        LobbyEvent::PhaseChanged(Phase::Launching),
        LobbyEvent::PhaseChanged(Phase::InMatch),
        LobbyEvent::PhaseChanged(Phase::HostPaused),
        LobbyEvent::PhaseChanged(Phase::Closed),
    ];
    for event in events {
        let bytes = event.encode().unwrap();
        assert_eq!(LobbyEvent::decode(&bytes).unwrap(), event);
        // Truncated and trailing records are refused.
        assert!(LobbyEvent::decode(&bytes[..bytes.len() - 1]).is_err() || bytes.len() == 1);
        let mut longer = bytes.clone();
        longer.push(0);
        assert!(LobbyEvent::decode(&longer).is_err());
    }
    assert!(LobbyEvent::decode(&[99]).is_err());
    assert!(LobbyEvent::decode(&[2, 0, 0]).is_err(), "peer 0 is no peer");
}

#[test]
fn accept_f55_bu1_a_client_cannot_send_before_the_session_grants_it() {
    let bind = SocketAddr::from((Ipv4Addr::LOCALHOST, 0));
    let _guard = crate::support::loopback::lock();
    let host = ServerSession::bind(
        SYNTHETIC_SESSION,
        synthetic_parameters(),
        bind,
        Duration::ZERO,
    )
    .unwrap();
    let mut client = ClientSession::connect_with_window(
        synthetic_hello(),
        host.local_addr().unwrap(),
        0xC2,
        Duration::ZERO,
        WINDOW,
    )
    .unwrap();
    assert_eq!(
        client.send_lobby(&chat(1, "early")),
        Err(ClientFault::NotInSession)
    );
}
