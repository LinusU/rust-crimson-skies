//! F55-B acceptance: the host's packet validation, readiness and membership
//! path.
//!
//! Minimum scenario (spec F55 AC02): a launch packet naming an old rules
//! revision is rejected. The rest pins the failure cases the acceptance
//! scenario implies — a client cannot use host authority, another session
//! epoch and a non-member never reach the state machine, a redelivered packet
//! is applied once, and a malformed buffer is refused before a single lobby
//! field is read — plus readiness and membership travelling the same
//! production entry point ([`Lobby::receive_bytes`]).
//!
//! Every test encodes a [`LobbyPacket`] with the production codec and hands
//! the *bytes* to the host, so the decode, epoch, replay, membership and
//! dispatch gates all run.

use std::collections::BTreeSet;

use cs_net::fixture::{SYNTHETIC_SESSION, synthetic_hello, synthetic_parameters};
use cs_net::lobby::*;
use cs_types::Tick;
use cs_types::content::{ContentId, ContentKind};
use cs_types::net::{EventId, PeerId, SessionId};

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

fn rules() -> LobbyRules {
    LobbyRules {
        scenario: id(ContentKind::MultiplayerScenario, "synthetic_duel"),
        banned: BTreeSet::new(),
        team_mode: TeamMode::FreeForAll,
        late_join: LateJoin::Closed,
    }
}

fn admission() -> Admission {
    Admission {
        access: AccessPolicy::Open,
        max_members: 4,
        host_loss: HostLoss::EndSession,
    }
}

fn open_lobby(admission: Admission) -> Lobby {
    Lobby::open(
        SYNTHETIC_SESSION,
        synthetic_parameters(),
        admission,
        rules(),
        callsign("Host"),
    )
    .unwrap()
}

fn join(lobby: &mut Lobby, name: &str) -> PeerId {
    lobby
        .admit(JoinRequest {
            hello: synthetic_hello(),
            callsign: callsign(name),
            password: None,
        })
        .unwrap()
        .0
}

fn loadout(components: &[&ContentId]) -> Loadout {
    Loadout {
        blueprint: id(ContentKind::Blueprint, "synthetic_zephyr"),
        components: components.iter().map(|c| (*c).clone()).collect(),
    }
}

fn cmd(lobby: &mut Lobby, from: PeerId, command: LobbyCommand) -> Result<Applied, LobbyError> {
    lobby.apply(from, command, &AcceptAll)
}

fn pick_and_ready(lobby: &mut Lobby, peer: PeerId, components: &[&ContentId]) {
    cmd(
        lobby,
        peer,
        LobbyCommand::Peer(PeerRequest::SetLoadout(loadout(components))),
    )
    .unwrap();
    let revision = lobby.revision();
    cmd(
        lobby,
        peer,
        LobbyCommand::Peer(PeerRequest::SetReady { revision }),
    )
    .unwrap();
}

/// Host + two clients, all ready; `cannon` is carried by the first client
/// only, `mg` by the host and the second.
fn ready_lobby() -> (Lobby, PeerId, PeerId, PeerId, ContentId, ContentId) {
    let mut lobby = open_lobby(admission());
    let host = lobby.host();
    let a = join(&mut lobby, "Ace");
    let b = join(&mut lobby, "Bee");
    let cannon = id(ContentKind::Gun, "synthetic_cannon");
    let mg = id(ContentKind::Gun, "synthetic_mg");
    pick_and_ready(&mut lobby, host, &[&mg]);
    pick_and_ready(&mut lobby, a, &[&cannon]);
    pick_and_ready(&mut lobby, b, &[&mg]);
    (lobby, host, a, b, cannon, mg)
}

/// One packet stamp for this session.
fn packet_id(sequence: u32) -> EventId {
    EventId {
        session: SYNTHETIC_SESSION,
        tick: Tick(3),
        producer: 7,
        sequence,
    }
}

/// The production path a member's packet takes: encode, decode, gates,
/// dispatch.
fn send(
    lobby: &mut Lobby,
    from: PeerId,
    packet: &LobbyPacket,
) -> Result<PacketOutcome, PacketReject> {
    let bytes = packet.encode().expect("the packet fits the wire caps");
    lobby.receive_bytes(from, &bytes, &AcceptAll)
}

/// The launch request naming the rules exactly as they are now.
fn live_launch(lobby: &Lobby) -> LaunchRequest {
    LaunchRequest {
        revision: lobby.revision(),
        digest: lobby.digest(),
    }
}

/// The digest of a lobby with a different scenario: a rules digest this
/// lobby does not hold.
fn other_rules_digest() -> RulesDigest {
    let mut other = open_lobby(admission());
    let other_host = other.host();
    cmd(
        &mut other,
        other_host,
        LobbyCommand::Host(HostAction::SetScenario(id(
            ContentKind::MultiplayerScenario,
            "synthetic_sweep",
        ))),
    )
    .unwrap();
    other.digest()
}

/// AC02, the minimum scenario: the host bans a weapon after the clients
/// ready, and the launch packet it built for the old revision is refused.
#[test]
fn accept_f55_b_launch_packet_for_an_old_rules_revision_is_rejected() {
    let (mut lobby, host, a, _b, cannon, mg) = ready_lobby();
    let old_revision = lobby.revision();
    let old_digest = lobby.digest();

    // The host bans the weapon Ace carries: revision 2, Ace unready with the
    // reason.
    let applied = cmd(
        &mut lobby,
        host,
        LobbyCommand::Host(HostAction::Ban(cannon.clone())),
    )
    .unwrap();
    assert!(applied.revocations().any(|revocation| revocation.peer == a));
    assert_eq!(lobby.revision().get(), old_revision.get() + 1);
    assert_ne!(old_digest, lobby.digest());

    // Ace re-readies under the live revision, with a legal machine gun.
    cmd(
        &mut lobby,
        a,
        LobbyCommand::Peer(PeerRequest::SetLoadout(loadout(&[&mg]))),
    )
    .unwrap();
    let live_revision = lobby.revision();
    cmd(
        &mut lobby,
        a,
        LobbyCommand::Peer(PeerRequest::SetReady {
            revision: live_revision,
        }),
    )
    .unwrap();

    // The launch packet built before the ban names the old revision.
    let stale = LobbyPacket::Launch {
        id: packet_id(1),
        request: LaunchRequest {
            revision: old_revision,
            digest: old_digest,
        },
    };
    let reject = send(&mut lobby, host, &stale).expect_err("an old revision must not launch");
    assert_eq!(
        reject,
        PacketReject::Launch(LaunchError::StaleRevision {
            current: lobby.revision(),
            found: old_revision,
        })
    );
    // Nothing half-happened: the lobby is still gathering, no launch pending.
    assert_eq!(lobby.phase(), Phase::Gathering);

    // The same host, the same session, the live revision: the packet is
    // accepted, which proves the refusal above was about the revision.
    let live = LobbyPacket::Launch {
        id: packet_id(2),
        request: live_launch(&lobby),
    };
    match send(&mut lobby, host, &live).unwrap() {
        PacketOutcome::Launch(LaunchProgress::Waiting { pending }) => {
            assert_eq!(pending.len(), 2);
        }
        other => panic!("expected a waiting launch, got {other:?}"),
    }
}

/// A launch commits only when every member has acknowledged the same
/// revision and digest; an acknowledgment naming other rules is refused
/// without disturbing the pending launch.
#[test]
fn accept_f55_b_a_launch_commits_only_after_every_member_acknowledges() {
    let (mut lobby, host, a, b, _cannon, _mg) = ready_lobby();
    let request = live_launch(&lobby);

    match send(
        &mut lobby,
        host,
        &LobbyPacket::Launch {
            id: packet_id(1),
            request,
        },
    )
    .unwrap()
    {
        PacketOutcome::Launch(LaunchProgress::Waiting { pending }) => {
            assert_eq!(
                pending,
                vec![a, b],
                "both clients still owe an acknowledgment"
            );
        }
        other => panic!("expected a waiting launch, got {other:?}"),
    }

    // An acknowledgment naming another rules digest changes nothing.
    let mismatched = LaunchRequest {
        revision: request.revision,
        digest: other_rules_digest(),
    };
    let reject = send(
        &mut lobby,
        a,
        &LobbyPacket::LaunchAck {
            id: packet_id(2),
            request: mismatched,
        },
    )
    .expect_err("an acknowledgment must name the pending rules");
    assert_eq!(reject, PacketReject::Launch(LaunchError::AckMismatch));
    assert_eq!(lobby.phase(), Phase::Launching);

    // Ace acknowledges: Bee still owes one. Bee acknowledges: the launch
    // commits atomically.
    match send(
        &mut lobby,
        a,
        &LobbyPacket::LaunchAck {
            id: packet_id(3),
            request,
        },
    )
    .unwrap()
    {
        PacketOutcome::Launch(LaunchProgress::Waiting { pending }) => {
            assert_eq!(pending, vec![b]);
        }
        other => panic!("expected the launch to wait for Bee, got {other:?}"),
    }
    match send(
        &mut lobby,
        b,
        &LobbyPacket::LaunchAck {
            id: packet_id(4),
            request,
        },
    )
    .unwrap()
    {
        PacketOutcome::Launch(LaunchProgress::Committed(order)) => {
            assert_eq!(order.revision, request.revision);
            assert_eq!(order.digest, request.digest);
            assert_eq!(order.members.len(), 3, "host and both clients");
        }
        other => panic!("expected a committed launch, got {other:?}"),
    }
    assert_eq!(lobby.phase(), Phase::InMatch);
}

/// The reliable channel may redeliver a packet: the id, not the payload, is
/// what the host deduplicates on, so a retry cannot apply a packet twice and
/// two genuinely distinct packets with the same text both apply.
#[test]
fn accept_f55_b_a_replayed_packet_is_applied_once() {
    let (mut lobby, _host, a, _b, _cannon, _mg) = ready_lobby();
    let chat = ChatText::new("on my way").unwrap();
    let packet = LobbyPacket::Command {
        id: packet_id(42),
        command: LobbyCommand::Peer(PeerRequest::Chat(chat.clone())),
    };

    let first = send(&mut lobby, a, &packet).unwrap();
    assert_eq!(first.events().len(), 1);
    assert!(matches!(
        &first.events()[0],
        LobbyEvent::Chat { from, text } if *from == a && *text == chat
    ));

    // The identical bytes arrive again: acknowledged, not re-applied.
    let replay = send(&mut lobby, a, &packet).unwrap();
    assert_eq!(replay, PacketOutcome::Replayed);
    assert!(replay.events().is_empty());

    // A different id with the same words is a different chat line.
    let second_line = send(
        &mut lobby,
        a,
        &LobbyPacket::Command {
            id: packet_id(43),
            command: LobbyCommand::Peer(PeerRequest::Chat(chat)),
        },
    )
    .unwrap();
    assert_eq!(second_line.events().len(), 1);
}

/// A redelivered launch packet after the commit is reported as already
/// applied instead of being refused as an illegal transition or re-committed.
#[test]
fn accept_f55_b_a_replayed_launch_packet_does_not_recommit() {
    let (mut lobby, host, a, b, _cannon, _mg) = ready_lobby();
    let request = live_launch(&lobby);
    let launch = LobbyPacket::Launch {
        id: packet_id(1),
        request,
    };

    send(&mut lobby, host, &launch).unwrap();
    for (sequence, peer) in [(2u32, a), (3u32, b)] {
        match send(
            &mut lobby,
            peer,
            &LobbyPacket::LaunchAck {
                id: packet_id(sequence),
                request,
            },
        )
        .unwrap()
        {
            PacketOutcome::Launch(_) => {}
            other => panic!("expected launch progress, got {other:?}"),
        }
    }
    assert_eq!(lobby.phase(), Phase::InMatch);

    let replay = send(&mut lobby, host, &launch).unwrap();
    assert_eq!(replay, PacketOutcome::Replayed);
    assert_eq!(lobby.phase(), Phase::InMatch);
}

/// Epoch mismatch rejects stale packets: a packet stamped with another
/// session is refused before its payload is read, whatever it asks.
#[test]
fn accept_f55_b_a_packet_from_another_session_epoch_is_refused() {
    let (mut lobby, host, _a, _b, _cannon, mg) = ready_lobby();
    let foreign_session = SessionId::new(4242).expect("nonzero");
    let foreign = |sequence: u32| EventId {
        session: foreign_session,
        tick: Tick(3),
        producer: 7,
        sequence,
    };

    let launch = LobbyPacket::Launch {
        id: foreign(1),
        request: live_launch(&lobby),
    };
    let reject = send(&mut lobby, host, &launch).expect_err("another epoch must not launch");
    assert_eq!(
        reject,
        PacketReject::ForeignSession {
            expected: SYNTHETIC_SESSION,
            found: foreign_session,
        }
    );
    assert_eq!(lobby.phase(), Phase::Gathering);

    let ban = LobbyPacket::Command {
        id: foreign(2),
        command: LobbyCommand::Host(HostAction::Ban(mg)),
    };
    let reject = send(&mut lobby, host, &ban).expect_err("another epoch must not change rules");
    assert_eq!(
        reject,
        PacketReject::ForeignSession {
            expected: SYNTHETIC_SESSION,
            found: foreign_session,
        }
    );
    assert_eq!(lobby.revision().get(), 1);
    assert!(lobby.rules().banned.is_empty());
}

/// Membership is checked before dispatch: a stranger, and a member who has
/// already left, cannot reach the state machine.
#[test]
fn accept_f55_b_a_non_member_packet_is_refused() {
    let mut lobby = open_lobby(admission());
    let host = lobby.host();
    let mg = id(ContentKind::Gun, "synthetic_mg");
    let stranger = PeerId::new(200).expect("nonzero");
    assert!(lobby.member(stranger).is_none());

    let ban = LobbyPacket::Command {
        id: packet_id(1),
        command: LobbyCommand::Host(HostAction::Ban(mg.clone())),
    };
    let reject = send(&mut lobby, stranger, &ban).expect_err("a stranger is not in the lobby");
    assert_eq!(reject, PacketReject::NotAMember { peer: stranger });
    assert_eq!(lobby.revision().get(), 1);
    assert!(lobby.rules().banned.is_empty());

    // A member who left cleanly keeps no access either.
    let a = join(&mut lobby, "Ace");
    assert!(matches!(
        lobby.remove_peer(a),
        Some(Departure::MemberLeft(_))
    ));
    let ban = LobbyPacket::Command {
        id: packet_id(2),
        command: LobbyCommand::Host(HostAction::Ban(mg)),
    };
    let reject = send(&mut lobby, a, &ban).expect_err("a departed member is not in the lobby");
    assert_eq!(reject, PacketReject::NotAMember { peer: a });
    assert_eq!(lobby.revision().get(), 1);
    assert_eq!(host, lobby.host(), "the host is unchanged");
}

/// No client can grant itself host authority or launch: the packet is
/// decoded, then refused by the authority check the dispatch runs first.
#[test]
fn accept_f55_b_a_client_packet_cannot_use_host_authority() {
    let (mut lobby, host, a, _b, cannon, mg) = ready_lobby();

    let launch = LobbyPacket::Launch {
        id: packet_id(1),
        request: live_launch(&lobby),
    };
    let reject = send(&mut lobby, a, &launch).expect_err("a client cannot launch");
    assert_eq!(
        reject,
        PacketReject::Launch(LaunchError::NotHost { peer: a })
    );
    assert_eq!(lobby.phase(), Phase::Gathering);

    let ban = LobbyPacket::Command {
        id: packet_id(2),
        command: LobbyCommand::Host(HostAction::Ban(cannon)),
    };
    let reject = send(&mut lobby, a, &ban).expect_err("a client cannot change the rules");
    assert_eq!(
        reject,
        PacketReject::Command(LobbyError::NotHost { peer: a })
    );
    assert_eq!(lobby.revision().get(), 1);
    assert!(lobby.rules().banned.is_empty());

    // The host's own packet over the same path is accepted.
    let ban = LobbyPacket::Command {
        id: packet_id(3),
        command: LobbyCommand::Host(HostAction::Ban(mg)),
    };
    match send(&mut lobby, host, &ban).unwrap() {
        PacketOutcome::Applied(applied) => assert!(
            applied
                .events
                .iter()
                .any(|event| matches!(event, LobbyEvent::RulesChanged { .. }))
        ),
        other => panic!("expected an applied command, got {other:?}"),
    }
    assert_eq!(lobby.revision().get(), 2);
}

/// Bytes that do not decode are refused as malformed before any lobby field
/// is read: the revision, the rules and the phase are exactly as they were.
#[test]
fn accept_f55_b_malformed_bytes_are_refused_before_the_lobby_changes() {
    let (mut lobby, host, _a, _b, _cannon, mg) = ready_lobby();
    let valid = LobbyPacket::Command {
        id: packet_id(1),
        command: LobbyCommand::Host(HostAction::Ban(mg)),
    }
    .encode()
    .unwrap();
    let unchanged = |lobby: &Lobby| {
        assert_eq!(lobby.revision().get(), 1);
        assert_eq!(lobby.phase(), Phase::Gathering);
        assert!(lobby.rules().banned.is_empty());
    };

    // Truncated: the buffer ends inside the record.
    let reject = lobby
        .receive_bytes(host, &valid[..10], &AcceptAll)
        .expect_err("a truncated packet is malformed");
    assert!(matches!(
        reject,
        PacketReject::Malformed(PacketError::Truncated { .. })
    ));
    unchanged(&lobby);

    // Trailing: one byte past a complete record.
    let mut trailing = valid.clone();
    trailing.push(0);
    let reject = lobby
        .receive_bytes(host, &trailing, &AcceptAll)
        .expect_err("a trailing byte is malformed");
    assert_eq!(
        reject,
        PacketReject::Malformed(PacketError::Trailing { len: 1 })
    );
    unchanged(&lobby);

    // Unknown packet tag.
    let mut unknown = valid.clone();
    unknown[0] = 9;
    let reject = lobby
        .receive_bytes(host, &unknown, &AcceptAll)
        .expect_err("an unknown tag is malformed");
    assert_eq!(
        reject,
        PacketReject::Malformed(PacketError::UnknownTag {
            field: "lobby_packet.tag",
            tag: 9
        })
    );
    unchanged(&lobby);

    // Unknown command kind: `command.kind` follows the tag and the 24-byte
    // event id.
    let mut unknown_kind = valid.clone();
    unknown_kind[25] = 9;
    let reject = lobby
        .receive_bytes(host, &unknown_kind, &AcceptAll)
        .expect_err("an unknown command kind is malformed");
    assert_eq!(
        reject,
        PacketReject::Malformed(PacketError::UnknownTag {
            field: "command.kind",
            tag: 9
        })
    );
    unchanged(&lobby);

    // A zero session id: only a nonzero id can name a live session.
    let mut zero = valid.clone();
    zero[1..9].fill(0);
    let reject = lobby
        .receive_bytes(host, &zero, &AcceptAll)
        .expect_err("a zero session id is malformed");
    assert_eq!(
        reject,
        PacketReject::Malformed(PacketError::ZeroId {
            field: "event.session"
        })
    );
    unchanged(&lobby);
}

/// Chat stays bounded, escaped text on the wire too: a line the lobby's own
/// constructors refuse never reaches the state machine.
#[test]
fn accept_f55_b_an_unacceptable_chat_packet_is_refused() {
    let (mut lobby, _host, a, _b, _cannon, _mg) = ready_lobby();

    let chat_packet = |chat: &[u8]| {
        let mut bytes = Vec::new();
        bytes.push(0); // tag: Command
        bytes.extend_from_slice(&SYNTHETIC_SESSION.get().to_le_bytes());
        bytes.extend_from_slice(&3u64.to_le_bytes()); // tick
        bytes.extend_from_slice(&7u32.to_le_bytes()); // producer
        bytes.extend_from_slice(&11u32.to_le_bytes()); // sequence
        bytes.push(1); // command kind: Peer
        bytes.push(5); // peer request: Chat
        bytes.extend_from_slice(&(chat.len() as u16).to_le_bytes());
        bytes.extend_from_slice(chat);
        bytes
    };

    // Not UTF-8.
    let reject = lobby
        .receive_bytes(a, &chat_packet(&[0x41, 0xFF]), &AcceptAll)
        .expect_err("binary chat is malformed");
    assert_eq!(
        reject,
        PacketReject::Malformed(PacketError::BadText {
            field: "peer_request.chat"
        })
    );

    // A control character: the lobby never carries one to a renderer.
    let reject = lobby
        .receive_bytes(a, &chat_packet(b"a\nb"), &AcceptAll)
        .expect_err("a control character is malformed");
    assert_eq!(
        reject,
        PacketReject::Malformed(PacketError::BadText {
            field: "peer_request.chat"
        })
    );

    // More characters than [`MAX_CHAT_CHARS`], still inside the byte cap:
    // the constructor counts characters.
    let reject = lobby
        .receive_bytes(a, &chat_packet(&vec![b'x'; 201]), &AcceptAll)
        .expect_err("an overlong chat line is malformed");
    assert_eq!(
        reject,
        PacketReject::Malformed(PacketError::BadText {
            field: "peer_request.chat"
        })
    );

    // More bytes than the wire cap carries.
    let reject = lobby
        .receive_bytes(
            a,
            &chat_packet(&vec![b'x'; MAX_CHAT_WIRE_BYTES + 1]),
            &AcceptAll,
        )
        .expect_err("an oversized chat line is malformed");
    assert_eq!(
        reject,
        PacketReject::Malformed(PacketError::TooLarge {
            field: "peer_request.chat",
            max: MAX_CHAT_WIRE_BYTES,
            len: MAX_CHAT_WIRE_BYTES + 1,
        })
    );

    // Nothing above reached the lobby: no chat was applied, and no packet id
    // joined the replay guard, so the well-formed line still goes through.
    let good = LobbyPacket::Command {
        id: packet_id(11),
        command: LobbyCommand::Peer(PeerRequest::Chat(ChatText::new("on my way").unwrap())),
    };
    match send(&mut lobby, a, &good).unwrap() {
        PacketOutcome::Applied(applied) => assert_eq!(applied.events.len(), 1),
        other => panic!("expected an applied chat, got {other:?}"),
    }
}

/// Membership and readiness travel the same production entry point: a client
/// is admitted, readies over packets, and loses readiness — with the reason —
/// when the host bans its weapon over a packet.
#[test]
fn accept_f55_b_readiness_and_membership_travel_the_host_receive_path() {
    let mut lobby = open_lobby(admission());
    let host = lobby.host();
    let cannon = id(ContentKind::Gun, "synthetic_cannon");

    // Membership: admitted through the handshake, announced to the lobby.
    let (a, admitted) = lobby
        .admit(JoinRequest {
            hello: synthetic_hello(),
            callsign: callsign("Ace"),
            password: None,
        })
        .unwrap();
    assert!(matches!(
        admitted.events.as_slice(),
        [LobbyEvent::MemberJoined { peer }] if *peer == a
    ));

    // Readiness over packets.
    match send(
        &mut lobby,
        a,
        &LobbyPacket::Command {
            id: packet_id(1),
            command: LobbyCommand::Peer(PeerRequest::SetLoadout(loadout(&[&cannon]))),
        },
    )
    .unwrap()
    {
        PacketOutcome::Applied(applied) => assert!(applied.events.is_empty()),
        other => panic!("expected an applied loadout, got {other:?}"),
    }
    let revision = lobby.revision();
    match send(
        &mut lobby,
        a,
        &LobbyPacket::Command {
            id: packet_id(2),
            command: LobbyCommand::Peer(PeerRequest::SetReady { revision }),
        },
    )
    .unwrap()
    {
        PacketOutcome::Applied(applied) => assert!(matches!(
            applied.events.as_slice(),
            [LobbyEvent::MemberReady { peer }] if *peer == a
        )),
        other => panic!("expected an applied ready, got {other:?}"),
    }
    assert_eq!(
        lobby.member(a).map(|member| member.ready),
        Some(Ready::Ready { revision })
    );

    // The host bans the weapon Ace carries, over a packet.
    let ban = LobbyPacket::Command {
        id: packet_id(3),
        command: LobbyCommand::Host(HostAction::Ban(cannon.clone())),
    };
    match send(&mut lobby, host, &ban).unwrap() {
        PacketOutcome::Applied(applied) => {
            assert!(
                applied
                    .events
                    .iter()
                    .any(|event| matches!(event, LobbyEvent::RulesChanged { .. }))
            );
            assert!(applied.revocations().any(|revocation| {
                revocation.peer == a
                    && revocation.reason
                        == RevokeReason::ComponentBanned {
                            component: cannon.clone(),
                        }
            }));
        }
        other => panic!("expected an applied ban, got {other:?}"),
    }
    assert_eq!(
        lobby.member(a).map(|member| member.ready),
        Some(Ready::NotReady),
        "the ban revoked readiness on the host record"
    );
}
