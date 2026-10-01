//! F55-A acceptance: lobby state and the rules-revision protocol.
//!
//! Minimum scenario (spec F55 AC01): the host bans a selected weapon after
//! clients ready; ready state is revoked and the reason is reported. The rest
//! pins AC02 (stale launch refused), AC03 (distinct join errors) and the
//! non-negotiable behaviours (authority, atomic launch, host loss).

use std::collections::BTreeSet;

use cs_net::compat::HandshakeReject;
use cs_net::fixture::{SYNTHETIC_SESSION, synthetic_hello, synthetic_parameters};
use cs_net::lobby::*;
use cs_types::content::{ContentId, ContentKind};
use cs_types::evidence::ContentHash;
use cs_types::net::PeerId;

struct AcceptAll;
impl LoadoutValidator for AcceptAll {
    fn validate(&self, _: &Loadout) -> Result<(), &'static str> {
        Ok(())
    }
}

struct RejectAll;
impl LoadoutValidator for RejectAll {
    fn validate(&self, _: &Loadout) -> Result<(), &'static str> {
        Err("over_budget")
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

/// Host + two clients, all ready; `cannon` is carried by the first client only.
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

#[test]
fn accept_f55_a_host_ban_revokes_ready_and_reports_reason() {
    let (mut lobby, host, a, b, cannon, _) = ready_lobby();
    let before = lobby.revision();
    let digest = lobby.digest();
    let applied = cmd(
        &mut lobby,
        host,
        LobbyCommand::Host(HostAction::Ban(cannon.clone())),
    )
    .unwrap();

    assert_eq!(lobby.revision().get(), before.get() + 1);
    assert_ne!(lobby.digest(), digest);
    let revocations: Vec<_> = applied.revocations().collect();
    assert_eq!(revocations.len(), 1, "only the carrier loses readiness");
    assert_eq!(revocations[0].peer, a);
    assert_eq!(
        revocations[0].reason,
        RevokeReason::ComponentBanned { component: cannon }
    );
    assert_eq!(
        revocations[0].reason.message_key(),
        "lobby.ready_revoked.component_banned"
    );
    assert_eq!(lobby.member(a).unwrap().ready, Ready::NotReady);
    // Unaffected members stay ready, carried to the new revision.
    for peer in [host, b] {
        assert_eq!(
            lobby.member(peer).unwrap().ready,
            Ready::Ready {
                revision: lobby.revision()
            }
        );
    }
    assert!(matches!(applied.events[0], LobbyEvent::RulesChanged { .. }));
}

#[test]
fn accept_f55_a_banned_loadout_cannot_ready_again() {
    let (mut lobby, host, a, _, cannon, _) = ready_lobby();
    cmd(
        &mut lobby,
        host,
        LobbyCommand::Host(HostAction::Ban(cannon.clone())),
    )
    .unwrap();
    let revision = lobby.revision();
    let refused = cmd(
        &mut lobby,
        a,
        LobbyCommand::Peer(PeerRequest::SetReady { revision }),
    );
    assert_eq!(
        refused,
        Err(LobbyError::LoadoutRejected(LoadoutProblem::Banned {
            component: cannon.clone()
        }))
    );
    let again = cmd(
        &mut lobby,
        a,
        LobbyCommand::Peer(PeerRequest::SetLoadout(loadout(&[&cannon]))),
    );
    assert!(matches!(again, Err(LobbyError::LoadoutRejected(_))));
}

#[test]
fn accept_f55_a_scenario_and_team_changes_revoke_everyone() {
    let (mut lobby, host, ..) = ready_lobby();
    let other = id(ContentKind::MultiplayerScenario, "synthetic_race");
    let applied = cmd(
        &mut lobby,
        host,
        LobbyCommand::Host(HostAction::SetScenario(other)),
    )
    .unwrap();
    assert_eq!(applied.revocations().count(), 3);
    assert!(
        applied
            .revocations()
            .all(|r| r.reason == RevokeReason::ScenarioChanged)
    );

    let (mut lobby, host, ..) = ready_lobby();
    let applied = cmd(
        &mut lobby,
        host,
        LobbyCommand::Host(HostAction::SetTeamMode(TeamMode::Teams { teams: 2 })),
    )
    .unwrap();
    assert_eq!(applied.revocations().count(), 3);
    assert!(lobby.members().all(|(_, m)| m.team.is_none()));
}

#[test]
fn accept_f55_a_unban_and_noop_do_not_revoke_and_noop_keeps_revision() {
    let (mut lobby, host, _, _, cannon, mg) = ready_lobby();
    cmd(
        &mut lobby,
        host,
        LobbyCommand::Host(HostAction::Ban(mg.clone())),
    )
    .unwrap();
    let revision = lobby.revision();
    // Banning twice changes nothing.
    let noop = cmd(
        &mut lobby,
        host,
        LobbyCommand::Host(HostAction::Ban(mg.clone())),
    )
    .unwrap();
    assert!(noop.events.is_empty());
    assert_eq!(lobby.revision(), revision);
    let applied = cmd(&mut lobby, host, LobbyCommand::Host(HostAction::Unban(mg))).unwrap();
    assert_eq!(applied.revocations().count(), 0);
    assert_eq!(lobby.revision().get(), revision.get() + 1);
    let _ = cannon;
}

#[test]
fn accept_f55_a_ready_on_a_stale_revision_is_refused() {
    let mut lobby = open_lobby(admission());
    let host = lobby.host();
    let a = join(&mut lobby, "Ace");
    cmd(
        &mut lobby,
        a,
        LobbyCommand::Peer(PeerRequest::SetLoadout(loadout(&[]))),
    )
    .unwrap();
    let old = lobby.revision();
    let gun = id(ContentKind::Gun, "synthetic_cannon");
    cmd(&mut lobby, host, LobbyCommand::Host(HostAction::Ban(gun))).unwrap();
    let refused = cmd(
        &mut lobby,
        a,
        LobbyCommand::Peer(PeerRequest::SetReady { revision: old }),
    );
    assert_eq!(
        refused,
        Err(LobbyError::StaleRevision {
            current: lobby.revision(),
            found: old
        })
    );
}

#[test]
fn accept_f55_a_clients_cannot_use_host_authority_or_launch() {
    let (mut lobby, host, a, ..) = ready_lobby();
    let gun = id(ContentKind::Gun, "synthetic_x");
    let before = (lobby.revision(), lobby.rules().clone());
    assert_eq!(
        cmd(&mut lobby, a, LobbyCommand::Host(HostAction::Ban(gun))),
        Err(LobbyError::NotHost { peer: a })
    );
    assert_eq!((lobby.revision(), lobby.rules().clone()), before);
    let request = LaunchRequest {
        revision: lobby.revision(),
        digest: lobby.digest(),
    };
    assert_eq!(
        lobby.begin_launch(a, request),
        Err(LaunchError::NotHost { peer: a })
    );
    assert_eq!(lobby.phase(), Phase::Gathering);
    let stranger = PeerId::new(99).unwrap();
    assert_eq!(
        cmd(
            &mut lobby,
            stranger,
            LobbyCommand::Host(HostAction::Unban(id(ContentKind::Gun, "q")))
        ),
        Err(LobbyError::UnknownPeer { peer: stranger })
    );
    let _ = host;
}

#[test]
fn accept_f55_a_launch_for_an_old_revision_is_rejected() {
    let (mut lobby, host, _, _, cannon, _) = ready_lobby();
    let stale = LaunchRequest {
        revision: lobby.revision(),
        digest: lobby.digest(),
    };
    cmd(
        &mut lobby,
        host,
        LobbyCommand::Host(HostAction::Unban(cannon.clone())),
    )
    .unwrap();
    // Unban was a no-op (never banned), so the request is still current.
    assert_eq!(lobby.revision(), stale.revision);
    cmd(
        &mut lobby,
        host,
        LobbyCommand::Host(HostAction::SetLateJoin(LateJoin::Open)),
    )
    .unwrap();
    assert_eq!(
        lobby.begin_launch(host, stale),
        Err(LaunchError::StaleRevision {
            current: lobby.revision(),
            found: stale.revision
        })
    );
    assert_eq!(lobby.phase(), Phase::Gathering);

    let current = LaunchRequest {
        revision: lobby.revision(),
        digest: lobby.digest(),
    };
    let bad_digest = LaunchRequest {
        digest: stale.digest,
        ..current
    };
    assert_eq!(
        lobby.begin_launch(host, bad_digest),
        Err(LaunchError::DigestMismatch)
    );
}

#[test]
fn accept_f55_a_launch_needs_everyone_ready_and_commits_atomically() {
    let mut lobby = open_lobby(admission());
    let host = lobby.host();
    let a = join(&mut lobby, "Ace");
    pick_and_ready(&mut lobby, host, &[]);
    let request = LaunchRequest {
        revision: lobby.revision(),
        digest: lobby.digest(),
    };
    assert_eq!(
        lobby.begin_launch(host, request),
        Err(LaunchError::NotReady { peers: vec![a] })
    );
    pick_and_ready(&mut lobby, a, &[]);
    let progress = lobby.begin_launch(host, request).unwrap();
    assert_eq!(progress, LaunchProgress::Waiting { pending: vec![a] });
    assert_eq!(lobby.phase(), Phase::Launching);
    // Rules and readiness are locked while a launch is pending.
    let gun = id(ContentKind::Gun, "g");
    assert_eq!(
        cmd(&mut lobby, host, LobbyCommand::Host(HostAction::Ban(gun))),
        Err(LobbyError::WrongPhase {
            phase: Phase::Launching
        })
    );
    // A wrong acknowledgment does not count.
    let wrong = LaunchRequest {
        digest: other_rules_digest(),
        ..request
    };
    assert_eq!(
        lobby.acknowledge_launch(a, wrong),
        Err(LaunchError::AckMismatch)
    );
    let LaunchProgress::Committed(order) = lobby.acknowledge_launch(a, request).unwrap() else {
        panic!("last acknowledgment commits");
    };
    assert_eq!(order.revision, request.revision);
    assert_eq!(order.digest, request.digest);
    assert_eq!(order.members.len(), 2);
    assert_eq!(lobby.phase(), Phase::InMatch);

    // Back to the lobby without reopening it (AC04 shape).
    lobby.finish_match().unwrap();
    assert_eq!(lobby.phase(), Phase::Gathering);
    assert!(lobby.members().all(|(_, m)| m.ready == Ready::NotReady));
}

#[test]
fn accept_f55_a_leaving_member_or_cancel_aborts_pending_launch() {
    let mut lobby = open_lobby(admission());
    let host = lobby.host();
    let a = join(&mut lobby, "Ace");
    pick_and_ready(&mut lobby, host, &[]);
    pick_and_ready(&mut lobby, a, &[]);
    let request = LaunchRequest {
        revision: lobby.revision(),
        digest: lobby.digest(),
    };
    lobby.begin_launch(host, request).unwrap();
    let Some(Departure::MemberLeft(applied)) = lobby.remove_peer(a) else {
        panic!("a client leaving is a clean departure");
    };
    assert!(applied.events.contains(&LobbyEvent::LaunchCancelled));
    assert_eq!(lobby.phase(), Phase::Gathering);
    assert_eq!(
        lobby.acknowledge_launch(a, request),
        Err(LaunchError::NoLaunchPending)
    );

    let b = join(&mut lobby, "Bee");
    pick_and_ready(&mut lobby, b, &[]);
    let request = LaunchRequest {
        revision: lobby.revision(),
        digest: lobby.digest(),
    };
    lobby.begin_launch(host, request).unwrap();
    assert_eq!(
        lobby.cancel_launch(b),
        Err(LaunchError::NotHost { peer: b })
    );
    lobby.cancel_launch(host).unwrap();
    assert_eq!(lobby.phase(), Phase::Gathering);
    assert_eq!(
        lobby.member(b).unwrap().ready,
        Ready::Ready {
            revision: lobby.revision()
        }
    );
}

#[test]
fn accept_f55_a_join_errors_are_distinct() {
    let secret = Secret::new("hunter2").unwrap();
    let mut lobby = open_lobby(Admission {
        access: AccessPolicy::Password(secret.clone()),
        max_members: 2,
        host_loss: HostLoss::EndSession,
    });
    let request = |password: Option<Secret>, hello| JoinRequest {
        hello,
        callsign: callsign("Ace"),
        password,
    };

    assert_eq!(
        lobby.admit(request(None, synthetic_hello())),
        Err(JoinError::WrongPassword)
    );
    assert_eq!(
        lobby.admit(request(Secret::new("nope"), synthetic_hello())),
        Err(JoinError::WrongPassword)
    );

    let mut mismatched = synthetic_hello();
    mismatched.compatibility.content_sha256 = ContentHash::from_bytes([1; 32]);
    let mismatch = lobby
        .admit(request(Some(secret.clone()), mismatched))
        .unwrap_err();
    assert!(matches!(
        mismatch,
        JoinError::ContentMismatch(HandshakeReject::ContentMismatch { .. })
    ));

    let (peer, _) = lobby
        .admit(request(Some(secret.clone()), synthetic_hello()))
        .unwrap();
    let full = lobby
        .admit(JoinRequest {
            hello: synthetic_hello(),
            callsign: callsign("Bee"),
            password: Some(secret.clone()),
        })
        .unwrap_err();
    assert_eq!(full, JoinError::LobbyFull { max: 2 });
    assert_ne!(JoinError::WrongPassword, full);
    assert_ne!(mismatch, full);

    // Refused joins spent no peer id: leaving frees the seat for the next id.
    lobby.remove_peer(peer);
    let (next, _) = lobby
        .admit(request(Some(secret), synthetic_hello()))
        .unwrap();
    assert_eq!(next.get(), peer.get() + 1);
}

#[test]
fn accept_f55_a_password_and_chat_never_leak_through_debug_or_markup() {
    let secret = Secret::new("hunter2").unwrap();
    assert!(!format!("{secret:?}").contains("hunter2"));
    let request = JoinRequest {
        hello: synthetic_hello(),
        callsign: callsign("Ace"),
        password: Some(secret),
    };
    assert!(!format!("{request:?}").contains("hunter2"));

    let chat = ChatText::new("<b>hi</b> & \"you\"").unwrap();
    assert_eq!(chat.as_str(), "<b>hi</b> & \"you\"");
    assert_eq!(
        chat.escaped(),
        "&lt;b&gt;hi&lt;/b&gt; &amp; &quot;you&quot;"
    );
    assert_eq!(ChatText::new(""), Err(LobbyError::InvalidChat));
    assert_eq!(ChatText::new("a\nb"), Err(LobbyError::InvalidChat));
    assert_eq!(
        ChatText::new(&"x".repeat(MAX_CHAT_CHARS + 1)),
        Err(LobbyError::InvalidChat)
    );
    assert!(ChatText::new(&"x".repeat(MAX_CHAT_CHARS)).is_ok());
    assert_eq!(Callsign::new(" Ace"), Err(LobbyError::InvalidCallsign));
    assert_eq!(
        Callsign::new(&"x".repeat(MAX_CALLSIGN_CHARS + 1)),
        Err(LobbyError::InvalidCallsign)
    );
}

#[test]
fn accept_f55_a_invalid_loadouts_and_late_join_policy() {
    let mut lobby = open_lobby(admission());
    let a = join(&mut lobby, "Ace");
    let gun = id(ContentKind::Gun, "g");
    let duplicate = Loadout {
        blueprint: id(ContentKind::Blueprint, "synthetic_zephyr"),
        components: vec![gun.clone(), gun],
    };
    assert_eq!(
        cmd(
            &mut lobby,
            a,
            LobbyCommand::Peer(PeerRequest::SetLoadout(duplicate))
        ),
        Err(LobbyError::LoadoutRejected(LoadoutProblem::Malformed))
    );
    // The shared validator has the last word.
    assert_eq!(
        lobby.apply(
            a,
            LobbyCommand::Peer(PeerRequest::SetLoadout(loadout(&[]))),
            &RejectAll
        ),
        Err(LobbyError::LoadoutRejected(LoadoutProblem::Invalid {
            code: "over_budget"
        }))
    );
    assert_eq!(lobby.member(a).unwrap().loadout, None);
    // Not-a-component bans and non-scenario scenarios are refused.
    let host = lobby.host();
    let mission = id(ContentKind::Mission, "m");
    assert_eq!(
        cmd(
            &mut lobby,
            host,
            LobbyCommand::Host(HostAction::Ban(mission.clone()))
        ),
        Err(LobbyError::NotAComponent {
            id: mission.clone()
        })
    );
    assert_eq!(
        cmd(
            &mut lobby,
            host,
            LobbyCommand::Host(HostAction::SetScenario(mission.clone()))
        ),
        Err(LobbyError::NotAScenario { id: mission })
    );
    assert_eq!(
        cmd(
            &mut lobby,
            host,
            LobbyCommand::Host(HostAction::SetTeamMode(TeamMode::Teams { teams: 1 }))
        ),
        Err(LobbyError::InvalidTeamMode)
    );
}

#[test]
fn accept_f55_a_late_join_and_host_loss_follow_the_declared_policy() {
    let mut lobby = open_lobby(admission());
    let host = lobby.host();
    pick_and_ready(&mut lobby, host, &[]);
    let request = LaunchRequest {
        revision: lobby.revision(),
        digest: lobby.digest(),
    };
    assert!(matches!(
        lobby.begin_launch(host, request).unwrap(),
        LaunchProgress::Committed(_)
    ));
    let late = || JoinRequest {
        hello: synthetic_hello(),
        callsign: callsign("Late"),
        password: None,
    };
    assert_eq!(lobby.admit(late()), Err(JoinError::LateJoinClosed));
    assert_eq!(lobby.remove_peer(host), Some(Departure::LobbyClosed));
    assert_eq!(lobby.phase(), Phase::Closed);

    let mut lobby = open_lobby(Admission {
        host_loss: HostLoss::PauseThenEnd { ticks: 600 },
        ..admission()
    });
    let host = lobby.host();
    assert_eq!(
        lobby.remove_peer(host),
        Some(Departure::LobbyPaused { ticks: 600 })
    );
    assert_eq!(lobby.phase(), Phase::HostPaused);
    assert_eq!(lobby.remove_peer(host), None);
}

#[test]
fn accept_f55_a_team_mode_requires_a_team_to_ready() {
    let mut lobby = open_lobby(admission());
    let host = lobby.host();
    let a = join(&mut lobby, "Ace");
    cmd(
        &mut lobby,
        host,
        LobbyCommand::Host(HostAction::SetTeamMode(TeamMode::Teams { teams: 2 })),
    )
    .unwrap();
    cmd(
        &mut lobby,
        a,
        LobbyCommand::Peer(PeerRequest::SetLoadout(loadout(&[]))),
    )
    .unwrap();
    let revision = lobby.revision();
    assert_eq!(
        cmd(
            &mut lobby,
            a,
            LobbyCommand::Peer(PeerRequest::SetReady { revision })
        ),
        Err(LobbyError::NoTeam)
    );
    assert_eq!(
        cmd(
            &mut lobby,
            a,
            LobbyCommand::Peer(PeerRequest::SetTeam(TeamId(2)))
        ),
        Err(LobbyError::TeamOutOfRange)
    );
    cmd(
        &mut lobby,
        a,
        LobbyCommand::Peer(PeerRequest::SetTeam(TeamId(1))),
    )
    .unwrap();
    cmd(
        &mut lobby,
        a,
        LobbyCommand::Peer(PeerRequest::SetReady { revision }),
    )
    .unwrap();
}

#[test]
fn accept_f55_a_digest_is_canonical_over_ban_order() {
    let a = id(ContentKind::Gun, "a");
    let b = id(ContentKind::Gun, "b");
    let mut one = rules();
    one.banned = [a.clone(), b.clone()].into_iter().collect();
    let mut two = rules();
    two.banned = [b, a].into_iter().collect();
    assert_eq!(one.digest(), two.digest());
    assert_ne!(one.digest(), rules().digest());
}

fn other_rules_digest() -> RulesDigest {
    let mut other = rules();
    other.late_join = LateJoin::Open;
    other.digest()
}
