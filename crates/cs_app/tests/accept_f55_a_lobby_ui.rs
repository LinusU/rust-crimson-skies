//! F55-A acceptance: the lobby screen displays the readiness-revocation reason
//! (spec F55 AC01) and keeps chat literal and mutable.

use std::collections::BTreeSet;

use cs_app::ui::lobby::{LobbyView, MAX_NOTICES, Notice};
use cs_net::fixture::{SYNTHETIC_SESSION, synthetic_hello, synthetic_parameters};
use cs_net::lobby::{
    AccessPolicy, Admission, Callsign, ChatText, HostAction, HostLoss, JoinRequest, LateJoin,
    Loadout, LoadoutValidator, Lobby, LobbyCommand, LobbyEvent, LobbyRules, PeerRequest,
    RevokeReason, TeamMode,
};
use cs_types::content::{ContentId, ContentKind};

struct Accept;
impl LoadoutValidator for Accept {
    fn validate(&self, _: &Loadout) -> Result<(), &'static str> {
        Ok(())
    }
}

fn id(kind: ContentKind, key: &str) -> ContentId {
    ContentId::from_source(kind, key).unwrap()
}

#[test]
fn accept_f55_a_ui_shows_reason_when_host_bans_a_ready_weapon() {
    let mut lobby = Lobby::open(
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
        Callsign::new("Host").unwrap(),
    )
    .unwrap();
    let host = lobby.host();
    let (client, _) = lobby
        .admit(JoinRequest {
            hello: synthetic_hello(),
            callsign: Callsign::new("Ace").unwrap(),
            password: None,
        })
        .unwrap();
    let cannon = id(ContentKind::Gun, "synthetic_cannon");
    let loadout = Loadout {
        blueprint: id(ContentKind::Blueprint, "synthetic_zephyr"),
        components: vec![cannon.clone()],
    };
    lobby
        .apply(
            client,
            LobbyCommand::Peer(PeerRequest::SetLoadout(loadout)),
            &Accept,
        )
        .unwrap();
    let revision = lobby.revision();
    lobby
        .apply(
            client,
            LobbyCommand::Peer(PeerRequest::SetReady { revision }),
            &Accept,
        )
        .unwrap();

    let applied = lobby
        .apply(
            host,
            LobbyCommand::Host(HostAction::Ban(cannon.clone())),
            &Accept,
        )
        .unwrap();
    let mut view = LobbyView::new();
    for event in &applied.events {
        view.observe(event);
    }
    let revoked = view
        .notices()
        .iter()
        .find_map(|notice| match notice {
            Notice::ReadyRevoked { peer, reason } => Some((*peer, reason.clone(), notice)),
            _ => None,
        })
        .expect("the revocation is displayed");
    assert_eq!(revoked.0, client);
    assert_eq!(
        revoked.1,
        RevokeReason::ComponentBanned { component: cannon }
    );
    assert!(revoked.2.fallback_text().contains("synthetic_cannon"));
    assert!(matches!(view.notices()[0], Notice::RulesChanged { .. }));
}

#[test]
fn accept_f55_a_ui_mutes_chat_and_keeps_markup_literal() {
    let loud = cs_types::net::PeerId::new(2).unwrap();
    let quiet = cs_types::net::PeerId::new(3).unwrap();
    let line = |from, text: &str| LobbyEvent::Chat {
        from,
        text: ChatText::new(text).unwrap(),
    };
    let mut view = LobbyView::new();
    view.mute(loud);
    assert!(view.is_muted(loud));
    view.observe(&line(loud, "spam"));
    view.observe(&line(quiet, "<script>x</script>"));
    assert_eq!(view.notices().len(), 1);
    assert_eq!(
        view.notices()[0].fallback_text(),
        "peer 3: <script>x</script>"
    );
    view.unmute(loud);
    view.observe(&line(loud, "back"));
    assert_eq!(view.notices().len(), 2);

    for n in 0..(MAX_NOTICES + 5) {
        view.observe(&line(quiet, &format!("line {n}")));
    }
    assert_eq!(view.notices().len(), MAX_NOTICES);
}
