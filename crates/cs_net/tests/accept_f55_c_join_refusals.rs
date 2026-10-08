//! F55-C acceptance: the join refusals a lobby screen displays are distinct
//! errors (spec F55 AC03) and every one of them has its own stable key and
//! its own line.
//!
//! The screen side of the same behaviour is
//! `crates/cs_app/tests/accept_f55_c_lobby_ui.rs`; this file pins
//! `cs_net::lobby::JoinError` itself, so the keys cannot collapse even if
//! the screen's mapping changes.

use std::collections::BTreeSet;

use cs_net::compat::HandshakeReject;
use cs_net::fixture::{SYNTHETIC_SESSION, synthetic_hello, synthetic_parameters};
use cs_net::lobby::{
    AccessPolicy, Admission, Callsign, HostLoss, JoinError, JoinRequest, LateJoin, Lobby,
    LobbyRules, Phase, Secret, TeamMode,
};
use cs_types::content::{ContentId, ContentKind};
use cs_types::evidence::ContentHash;

fn scenario() -> ContentId {
    ContentId::from_source(ContentKind::MultiplayerScenario, "synthetic_duel").unwrap()
}

#[test]
fn accept_f55_c_the_three_named_join_refusals_are_distinct_by_key_and_line() {
    let secret = Secret::new("hunter2").unwrap();
    let mut lobby = Lobby::open(
        SYNTHETIC_SESSION,
        synthetic_parameters(),
        Admission {
            access: AccessPolicy::Password(secret.clone()),
            max_members: 2,
            host_loss: HostLoss::EndSession,
        },
        LobbyRules {
            scenario: scenario(),
            banned: BTreeSet::new(),
            team_mode: TeamMode::FreeForAll,
            late_join: LateJoin::Closed,
        },
        Callsign::new("Host").unwrap(),
    )
    .unwrap();

    let wrong = lobby
        .admit(JoinRequest {
            hello: synthetic_hello(),
            callsign: Callsign::new("Ace").unwrap(),
            password: None,
        })
        .unwrap_err();

    let mut foreign = synthetic_hello();
    foreign.compatibility.content_sha256 = ContentHash::from_bytes([1; 32]);
    let mismatch = lobby
        .admit(JoinRequest {
            hello: foreign,
            callsign: Callsign::new("Ace").unwrap(),
            password: Some(secret.clone()),
        })
        .unwrap_err();

    let (seated, _) = lobby
        .admit(JoinRequest {
            hello: synthetic_hello(),
            callsign: Callsign::new("Ace").unwrap(),
            password: Some(secret.clone()),
        })
        .unwrap();

    let full = lobby
        .admit(JoinRequest {
            hello: synthetic_hello(),
            callsign: Callsign::new("Bee").unwrap(),
            password: Some(secret),
        })
        .unwrap_err();

    assert_eq!(wrong, JoinError::WrongPassword);
    assert!(matches!(
        mismatch,
        JoinError::ContentMismatch(HandshakeReject::ContentMismatch { .. })
    ));
    assert_eq!(full, JoinError::LobbyFull { max: 2 });
    assert_eq!(lobby.members().count(), 2, "the refusals seated nobody");

    let refusals = [&wrong, &mismatch, &full];
    let keys: BTreeSet<&'static str> = refusals.iter().map(|error| error.message_key()).collect();
    assert_eq!(
        keys,
        BTreeSet::from([
            "lobby.join.wrong_password",
            "lobby.join.content_mismatch",
            "lobby.join.full",
        ])
    );
    let lines: BTreeSet<String> = refusals.iter().map(|error| error.to_string()).collect();
    assert_eq!(lines.len(), 3, "three refusals, three different lines");

    lobby.remove_peer(seated);
}

#[test]
fn accept_f55_c_every_join_refusal_has_its_own_stable_key_and_line() {
    let refusals = [
        JoinError::WrongPassword,
        JoinError::LobbyFull { max: 2 },
        JoinError::ContentMismatch(HandshakeReject::SessionFull { max: 32 }),
        JoinError::LateJoinClosed,
        JoinError::NotAccepting {
            phase: Phase::InMatch,
        },
        JoinError::CallsignTaken,
    ];

    let keys: BTreeSet<&'static str> = refusals.iter().map(|error| error.message_key()).collect();
    assert_eq!(keys.len(), refusals.len(), "every refusal has its own key");
    for key in &keys {
        assert!(key.starts_with("lobby.join."), "unnamespaced key {key}");
    }

    let lines: BTreeSet<String> = refusals.iter().map(|error| error.to_string()).collect();
    assert_eq!(
        lines.len(),
        refusals.len(),
        "every refusal has its own line"
    );
    for line in &lines {
        assert!(!line.is_empty());
    }

    // A screen that reports a refusal generically still has a `Display` and
    // an `Error` to fall back on.
    let first: &dyn std::error::Error = &refusals[0];
    assert!(!first.to_string().is_empty());
}
