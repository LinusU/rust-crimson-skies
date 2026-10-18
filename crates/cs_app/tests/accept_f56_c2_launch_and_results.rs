//! F56-C2 acceptance: the lobby launch consumes the F56 match rules and the
//! multiplayer results screen consumes the finished match.
//!
//! Spec: `specs/F56-original-multiplayer-scenarios-and-mode-rules.md`, stage
//! `### F56-C`; contract `docs/contracts/UI-NETWORK.md`. Everything calls
//! production code (`MatchLauncher`, `ResultsScreen`, `LobbyLink`,
//! `Lobby`, `RuleDraft`, `MatchSession`). The map slots, limits and score
//! values are authored test inputs: the original per-mode values are unknown
//! (F56-A findings) and block a production launch.

use std::collections::BTreeSet;

use cs_app::ui::lobby::{
    HostSettings, LaunchInputs, LaunchRefusal, LaunchStep, LobbyLink, MatchLauncher, ResultsScreen,
};
use cs_content::multiplayer::{ScenarioSlot, SlotCatalog, SlotObjective, TARGET_DESCRIPTION_FLAG};
use cs_net::fixture::{SYNTHETIC_SESSION, synthetic_hello, synthetic_parameters};
use cs_net::lobby::{
    AccessPolicy, Admission, Callsign, HostAction, HostLoss, JoinRequest, LateJoin, LaunchError,
    Loadout, LoadoutValidator, LobbyCommand, LobbyRules, PeerRequest, Phase, TeamMode,
};
use cs_net::rules::{
    CustomPlanes, DisconnectPolicy, HumanRange, HumanScaling, Limit, Lives, ResolveError, Respawn,
    RuleDraft, Spawn, StartError, Victory,
};
use cs_sim::multiplayer::result::{LethalEvent, LethalKind, ScoreTable, Side};
use cs_sim::multiplayer::session::SessionError;
use cs_types::Tick;
use cs_types::asset_id::SourceSpan;
use cs_types::content::{ContentId, ContentKind, Known, Provenance, Resolved};
use cs_types::evidence::{ClaimId, ClaimStatus, ContentHash};
use cs_types::net::{EventId, PeerId, SessionId};

struct Accept;
impl LoadoutValidator for Accept {
    fn validate(&self, _: &Loadout) -> Result<(), &'static str> {
        Ok(())
    }
}

fn claim() -> ClaimId {
    ClaimId::new("multiplayer.slot.objectives.fixture").expect("a claim id")
}

fn slot(id: &str, number: u8, objectives: Resolved<Vec<SlotObjective>>) -> ScenarioSlot {
    ScenarioSlot {
        id: ContentId::parse(id).expect("a scenario id"),
        world_group: "C1".to_owned(),
        slot: number,
        program: SourceSpan::new(
            ContentHash::from_bytes([7_u8; 32]),
            &format!("ZBD/C1/MP{number}/zrdr.zbd"),
            None,
            0,
            64,
            None,
        )
        .expect("a source span"),
        companions: Vec::new(),
        markers: Vec::new(),
        mode: Resolved::unknown(claim(), "the fixture states no mode").expect("a reason"),
        objectives,
    }
}

fn flags() -> Resolved<Vec<SlotObjective>> {
    let records = (0..2)
        .map(|index| SlotObjective {
            index,
            description: Some(TARGET_DESCRIPTION_FLAG.to_owned()),
            nodes: vec![format!("node{index}")],
            help_label: None,
            category_label: None,
            directives: vec!["objective".to_owned()],
        })
        .collect();
    Resolved::Known(Known::new(
        records,
        Provenance {
            claim_id: claim(),
            class: ClaimStatus::Designed,
            source: None,
        },
    ))
}

const SHIPPED: &str = "multiplayer_scenario/slot.c1.mp2";
const UNDECODED: &str = "multiplayer_scenario/slot.c1.mp3";
const NOT_SHIPPED: &str = "multiplayer_scenario/slot.c1.mp9";

fn catalog() -> SlotCatalog {
    SlotCatalog {
        slots: vec![
            slot(SHIPPED, 2, flags()),
            slot(
                UNDECODED,
                3,
                Resolved::unknown(claim(), "targets.zrd absent").expect("a reason"),
            ),
        ],
        without_program: Vec::new(),
    }
}

fn measured() -> RuleDraft {
    RuleDraft {
        time_limit: Some(Limit::At(600)),
        score_limit: Some(Limit::At(2)),
        lives: Some(Lives::Unlimited),
        respawn: Some(Respawn::AfterTicks(180)),
        spawn: Some(Spawn::OwnSide),
        friendly_fire: Some(false),
        victory: Some(Victory::HighestScore),
        disconnect: Some(DisconnectPolicy::KeepScore),
        humans: Some(HumanRange { min: 2, max: 4 }),
        human_scaling: Some(HumanScaling::None),
        custom_planes: Some(CustomPlanes::Forbidden),
        component_limit: Some(Limit::At(10)),
        ..RuleDraft::default()
    }
}

fn loadout() -> Loadout {
    Loadout {
        blueprint: ContentId::from_source(ContentKind::Blueprint, "synthetic_zephyr")
            .expect("a content id"),
        components: Vec::new(),
    }
}

fn generation(n: u64) -> SessionId {
    SessionId::new(n).expect("a session id")
}

fn inputs<'a>(
    catalog: &'a SlotCatalog,
    measured: RuleDraft,
    gen_n: u64,
    is_custom: &'a dyn Fn(&Loadout) -> bool,
) -> LaunchInputs<'a> {
    LaunchInputs {
        catalog,
        measured,
        table: ScoreTable {
            kill: 1,
            crash: -1,
            team_kill: -1,
        },
        generation: generation(gen_n),
        is_custom,
    }
}

/// A free-for-all lobby on `scenario` with the host and one guest, both
/// ready; returns the link, host and guest.
fn ready_lobby(scenario: &str) -> (LobbyLink, PeerId, PeerId) {
    let mut link = LobbyLink::host(
        HostSettings {
            session: SYNTHETIC_SESSION,
            params: synthetic_parameters(),
            admission: Admission {
                access: AccessPolicy::Open,
                max_members: 4,
                host_loss: HostLoss::EndSession,
            },
            rules: LobbyRules {
                scenario: ContentId::parse(scenario).expect("a scenario id"),
                banned: BTreeSet::new(),
                team_mode: TeamMode::FreeForAll,
                late_join: LateJoin::Closed,
            },
            host_callsign: Callsign::new("Host").expect("a callsign"),
        },
        Box::new(Accept),
    )
    .expect("the lobby opens");
    let host = link.host_peer();
    let (guest, _) = link
        .join(JoinRequest {
            hello: synthetic_hello(),
            callsign: Callsign::new("Ace").expect("a callsign"),
            password: None,
        })
        .expect("the guest joins");
    ready_everyone(&mut link, &[host, guest]);
    (link, host, guest)
}

fn ready_everyone(link: &mut LobbyLink, peers: &[PeerId]) {
    for peer in peers {
        link.command(
            *peer,
            LobbyCommand::Peer(PeerRequest::SetLoadout(loadout())),
        )
        .expect("a loadout");
        let revision = link.revision();
        link.command(
            *peer,
            LobbyCommand::Peer(PeerRequest::SetReady { revision }),
        )
        .expect("ready");
    }
}

fn never_custom(_: &Loadout) -> bool {
    false
}

fn launch(
    launcher: &mut MatchLauncher,
    link: &mut LobbyLink,
    guest: PeerId,
    catalog: &SlotCatalog,
    gen_n: u64,
) {
    let custom: &dyn Fn(&Loadout) -> bool = &never_custom;
    let inputs = inputs(catalog, measured(), gen_n, custom);
    match launcher.begin(link, &inputs).expect("the launch begins") {
        LaunchStep::Waiting(pending) => assert_eq!(pending, vec![guest]),
        LaunchStep::Started => panic!("the guest has not acknowledged"),
    }
    assert!(matches!(
        launcher.acknowledge(link, guest).expect("ack"),
        LaunchStep::Started
    ));
}

fn lethal(session: SessionId, tick: u64, killer: PeerId, victim: PeerId, seq: u32) -> LethalEvent {
    LethalEvent {
        id: EventId {
            session,
            tick: Tick(tick),
            producer: 1,
            sequence: seq,
        },
        victim,
        kind: LethalKind::Kill { killer },
    }
}

#[test]
fn accept_f56_c2_host_launch_starts_exactly_one_session_from_the_plan() {
    let catalog = catalog();
    let (mut link, host, guest) = ready_lobby(SHIPPED);
    let mut launcher = MatchLauncher::new();
    launch(&mut launcher, &mut link, guest, &catalog, 10);

    assert_eq!(link.lobby().phase(), Phase::InMatch);
    let session = launcher.session().expect("one session");
    assert_eq!(session.session(), generation(10));
    assert_eq!(session.scenario().as_str(), SHIPPED);
    assert_eq!(session.objectives(), 2, "the slot's possessable objectives");
    assert_eq!(session.limits().time_limit, Some(Tick(600)));
    assert_eq!(session.limits().score_limit, Some(2));
    assert!(session.roster().contains(host) && session.roster().contains(guest));

    // A second launch cannot start a second session.
    let custom: &dyn Fn(&Loadout) -> bool = &never_custom;
    let again = launcher.begin(&mut link, &inputs(&catalog, measured(), 11, custom));
    assert_eq!(again.unwrap_err(), LaunchRefusal::Busy);
    assert_eq!(launcher.session().unwrap().session(), generation(10));
}

#[test]
fn accept_f56_c2_a_launch_the_rules_refuse_never_reaches_the_lobby() {
    let catalog = catalog();
    let custom: &dyn Fn(&Loadout) -> bool = &never_custom;

    // An id the installation does not ship.
    let (mut link, _, _) = ready_lobby(NOT_SHIPPED);
    let mut launcher = MatchLauncher::new();
    let refusal = launcher
        .begin(&mut link, &inputs(&catalog, measured(), 10, custom))
        .unwrap_err();
    assert_eq!(
        refusal,
        LaunchRefusal::UnknownScenario(ContentId::parse(NOT_SHIPPED).unwrap())
    );
    assert_eq!(link.lobby().phase(), Phase::Gathering, "nothing committed");

    // A slot whose objective records do not decode.
    let (mut link, _, _) = ready_lobby(UNDECODED);
    let refusal = launcher
        .begin(&mut link, &inputs(&catalog, measured(), 10, custom))
        .unwrap_err();
    assert!(matches!(refusal, LaunchRefusal::ObjectivesUnknown(_)));
    assert_eq!(link.lobby().phase(), Phase::Gathering);

    // Production rules are unknown: the launch is blocked, naming the fields.
    let (mut link, _, _) = ready_lobby(SHIPPED);
    let refusal = launcher
        .begin(
            &mut link,
            &inputs(&catalog, RuleDraft::default(), 10, custom),
        )
        .unwrap_err();
    match refusal {
        LaunchRefusal::Rules(ResolveError::Blocked(blocked)) => {
            assert!(!blocked.missing.is_empty());
        }
        other => panic!("unknown rules must block by name: {other:?}"),
    }
    assert_eq!(link.lobby().phase(), Phase::Gathering);

    // validate_start: the mode admits 2..=4 humans; a lone host is refused.
    let mut solo = LobbyLink::host(
        HostSettings {
            session: SYNTHETIC_SESSION,
            params: synthetic_parameters(),
            admission: Admission {
                access: AccessPolicy::Open,
                max_members: 4,
                host_loss: HostLoss::EndSession,
            },
            rules: LobbyRules {
                scenario: ContentId::parse(SHIPPED).unwrap(),
                banned: BTreeSet::new(),
                team_mode: TeamMode::FreeForAll,
                late_join: LateJoin::Closed,
            },
            host_callsign: Callsign::new("Host").unwrap(),
        },
        Box::new(Accept),
    )
    .unwrap();
    let host = solo.host_peer();
    ready_everyone(&mut solo, &[host]);
    let refusal = launcher
        .begin(&mut solo, &inputs(&catalog, measured(), 10, custom))
        .unwrap_err();
    assert_eq!(
        refusal,
        LaunchRefusal::Start(StartError::TooFewHumans { got: 1, min: 2 })
    );
    assert_eq!(solo.lobby().phase(), Phase::Gathering);

    // A custom plane in a stock-only mode.
    let (mut link, _, _) = ready_lobby(SHIPPED);
    let all_custom: &dyn Fn(&Loadout) -> bool = &|_| true;
    let refusal = launcher
        .begin(&mut link, &inputs(&catalog, measured(), 10, all_custom))
        .unwrap_err();
    assert_eq!(
        refusal,
        LaunchRefusal::Start(StartError::CustomPlanesForbidden)
    );

    // A lobby that is not ready is refused by the lobby itself, and the
    // launcher keeps no pending launch.
    let (mut link, host, _) = ready_lobby(SHIPPED);
    link.command(host, LobbyCommand::Peer(PeerRequest::Unready))
        .unwrap();
    let refusal = launcher
        .begin(&mut link, &inputs(&catalog, measured(), 10, custom))
        .unwrap_err();
    assert!(matches!(
        refusal,
        LaunchRefusal::Lobby(LaunchError::NotReady { .. })
    ));
    assert!(!launcher.is_pending() && launcher.session().is_none());
}

#[test]
fn accept_f56_c2_results_show_the_match_and_back_to_lobby_leaves_it_behind() {
    let catalog = catalog();
    let (mut link, host, guest) = ready_lobby(SHIPPED);
    let mut launcher = MatchLauncher::new();
    launch(&mut launcher, &mut link, guest, &catalog, 10);

    assert!(
        ResultsScreen::from_launcher(&launcher).is_none(),
        "no results while the match runs"
    );
    // Back before the result is sealed is refused.
    assert_eq!(launcher.finish(&mut link).unwrap_err(), LaunchRefusal::Busy);

    // The host downs the guest twice: the score limit (2) ends the match.
    let first = generation(10);
    launcher
        .session_mut()
        .unwrap()
        .submit_lethal(lethal(first, 5, host, guest, 0))
        .unwrap();
    launcher
        .session_mut()
        .unwrap()
        .submit_lethal(lethal(first, 6, host, guest, 1))
        .unwrap();
    assert!(launcher.session_mut().unwrap().close_tick(Tick(6)).ended);

    let screen = ResultsScreen::from_launcher(&launcher).expect("results");
    assert_eq!(screen.reason_key(), "match.end.score_limit");
    assert_eq!(screen.outcome_key(), "match.outcome.win");
    assert_eq!(screen.record().winner(), Some(Side::Participant(host)));
    assert_eq!(screen.standings()[0].score, 2);
    assert!(screen.fallback_lines()[0].contains("score limit"));
    assert!(matches!(
        launcher
            .session_mut()
            .unwrap()
            .submit_lethal(lethal(first, 7, host, guest, 2)),
        Err(SessionError::MatchOver)
    ));

    screen
        .back_to_lobby(&mut launcher, &mut link)
        .expect("results -> lobby");
    assert!(launcher.session().is_none(), "the finished match is gone");
    assert_eq!(link.lobby().phase(), Phase::Gathering);
    assert!(
        link.lobby()
            .members()
            .all(|(_, m)| m.ready == cs_net::lobby::Ready::NotReady)
    );

    // The next match is a new generation: no score, no timer, no stale record.
    ready_everyone(&mut link, &[host, guest]);
    launch(&mut launcher, &mut link, guest, &catalog, 11);
    let next = launcher.session().unwrap();
    assert_eq!(next.session(), generation(11));
    assert_eq!(next.score(Side::Participant(host)), Some(0));
    assert_eq!(next.clock(), None);
    assert!(next.end_of_match().is_none());
    assert!(ResultsScreen::from_launcher(&launcher).is_none());
    // The old generation's events are refused by the new session.
    assert!(
        launcher
            .session_mut()
            .unwrap()
            .submit_lethal(lethal(first, 3, host, guest, 5))
            .is_err()
    );
}

#[test]
fn accept_f56_c2_a_cancelled_launch_drops_the_plan_and_changing_hosts_options_is_judged() {
    let catalog = catalog();
    let custom: &dyn Fn(&Loadout) -> bool = &never_custom;
    let (mut link, host, _) = ready_lobby(SHIPPED);
    let mut launcher = MatchLauncher::new();
    assert!(matches!(
        launcher
            .begin(&mut link, &inputs(&catalog, measured(), 10, custom))
            .unwrap(),
        LaunchStep::Waiting(_)
    ));
    assert!(launcher.is_pending());
    launcher.cancel(&mut link).expect("cancel");
    assert!(!launcher.is_pending() && launcher.session().is_none());
    assert_eq!(link.lobby().phase(), Phase::Gathering);

    // The host retargets the map to one the installation does not ship: the
    // retry is refused by name even though everyone was ready before.
    link.command(
        host,
        LobbyCommand::Host(HostAction::SetScenario(
            ContentId::parse(NOT_SHIPPED).unwrap(),
        )),
    )
    .unwrap();
    assert!(matches!(
        launcher.begin(&mut link, &inputs(&catalog, measured(), 10, custom)),
        Err(LaunchRefusal::UnknownScenario(_))
    ));
}
