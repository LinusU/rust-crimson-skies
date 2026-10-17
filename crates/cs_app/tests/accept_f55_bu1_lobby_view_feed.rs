//! F55-BU1-WIRE acceptance: the host's lobby events, carried on the F54
//! session wire, reach the client's `LobbyView`.

use std::collections::BTreeSet;
use std::net::{Ipv4Addr, SocketAddr};
use std::time::Duration;

use cs_app::ui::lobby::{LobbyView, Notice};
use cs_net::fixture::{SYNTHETIC_SESSION, synthetic_hello, synthetic_parameters};
use cs_net::lifecycle::{ClientNotice, ClientSession, ServerSession};
use cs_net::lobby::*;
use cs_net::message::LobbyRefusal;
use cs_net::transport::ConnectWindow;
use cs_types::Tick;
use cs_types::content::{ContentId, ContentKind};
use cs_types::net::EventId;

struct AcceptAll;

impl LoadoutValidator for AcceptAll {
    fn validate(&self, _: &Loadout) -> Result<(), &'static str> {
        Ok(())
    }
}

fn packet_id(sequence: u32) -> EventId {
    EventId {
        session: SYNTHETIC_SESSION,
        tick: Tick(1),
        producer: 5,
        sequence,
    }
}

#[test]
fn accept_f55_bu1_wire_events_and_refusals_reach_the_lobby_view() {
    let step = Duration::from_millis(16);
    let window = ConnectWindow::new(120);
    let mut attempt = 0;
    let (mut host, mut client) = loop {
        attempt += 1;
        assert!(attempt <= 24, "the loopback never carried a handshake");
        let bind = SocketAddr::from((Ipv4Addr::LOCALHOST, 0));
        let mut host = ServerSession::bind(
            SYNTHETIC_SESSION,
            synthetic_parameters(),
            bind,
            Duration::ZERO,
        )
        .unwrap();
        let mut client = ClientSession::connect_with_window(
            synthetic_hello(),
            host.local_addr().unwrap(),
            0xC1,
            Duration::ZERO,
            window,
        )
        .unwrap();
        for _ in 0..4_000 {
            client.pump(step);
            host.pump(step);
            if client.grant().is_some() && host.members().next().is_some() {
                break;
            }
        }
        if client.grant().is_some() {
            break (host, client);
        }
    };
    let peer = host.members().next().unwrap();

    let mut lobby = Lobby::open(
        SYNTHETIC_SESSION,
        synthetic_parameters(),
        Admission {
            access: AccessPolicy::Open,
            max_members: 4,
            host_loss: HostLoss::EndSession,
        },
        LobbyRules {
            scenario: ContentId::from_source(ContentKind::MultiplayerScenario, "synthetic_duel")
                .unwrap(),
            banned: BTreeSet::new(),
            team_mode: TeamMode::FreeForAll,
            late_join: LateJoin::Closed,
        },
        Callsign::new("Host").unwrap(),
    )
    .unwrap();
    let (member, _) = lobby
        .admit(JoinRequest {
            hello: synthetic_hello(),
            callsign: Callsign::new("Ace").unwrap(),
            password: None,
        })
        .unwrap();
    host.attach_lobby(lobby, Box::new(AcceptAll)).unwrap();
    host.bind_lobby_member(peer, member).unwrap();

    let mut view = LobbyView::new();
    let mut seen = Vec::new();
    let mut pump_until = |host: &mut ServerSession,
                          client: &mut ClientSession,
                          view: &mut LobbyView,
                          done: &dyn Fn(&LobbyView) -> bool| {
        for _ in 0..4_000 {
            if done(view) {
                return;
            }
            let notices = client.pump(step);
            view.observe_session(&notices);
            seen.extend(notices);
            host.pump(step);
        }
        panic!("the view never changed: {seen:?}");
    };

    // A member's chat crosses the wire and shows up in the view.
    client
        .send_lobby(&LobbyPacket::Command {
            id: packet_id(1),
            command: LobbyCommand::Peer(PeerRequest::Chat(ChatText::new("ready room").unwrap())),
        })
        .unwrap();
    pump_until(&mut host, &mut client, &mut view, &|view| {
        view.notices().iter().any(|notice| {
            matches!(notice, Notice::Chat { from, text }
                if *from == member && text.as_str() == "ready room")
        })
    });

    // A launch from a non-host member is refused; the player is told.
    client
        .send_lobby(&LobbyPacket::Launch {
            id: packet_id(2),
            request: LaunchRequest {
                revision: Revision::FIRST,
                digest: host.lobby().unwrap().digest(),
            },
        })
        .unwrap();
    pump_until(&mut host, &mut client, &mut view, &|view| {
        view.notices()
            .iter()
            .any(|notice| matches!(notice, Notice::PacketRefused { .. }))
    });
    assert!(view.notices().iter().any(|notice| matches!(
        notice,
        Notice::PacketRefused {
            refusal: LobbyRefusal::Launch
        }
    )));
    assert!(
        seen.iter()
            .all(|notice| !matches!(notice, ClientNotice::Dropped { .. })),
        "{seen:?}"
    );
}
