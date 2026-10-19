//! F55-CU1-ENTRY acceptance (#787): the F55 lobby screen reached from the
//! front-end state table. Task test prefix: `accept_f55_cu1_`.
//!
//! Shared contract: `docs/contracts/UI-NETWORK.md` ("Use an explicit state
//! table for every screen and Back/Cancel path", "Focus and accessibility
//! state reset sensibly on entry").
//!
//! The minimum scenario is
//! `accept_f55_cu1_the_lobby_is_reached_from_the_main_menu_and_back_discards_the_drafts`:
//! from the main menu, by clicking authored buttons, into the lobby and out
//! of it again — the drafts the forms held are gone. The rest pin the room's
//! leave-first Back, the application-supplied inputs the forms submit with
//! (session epoch and compatibility signature for `submit_lobby_host`,
//! resolved endpoint and client hello for `submit_lobby_join`, and the
//! shared `LoadoutValidator`), the entry refusal when they are missing, the
//! wiring refusals off the screen, and the table's totality with the new
//! rows.
//!
//! Everything here calls production code: `FrontEndFlow::press` /
//! `click` / `lobby_back` over `super::TABLE`, `FlowDomain`'s `open_lobby`
//! and `lobby_back`, and `LobbyScreen` / `LobbyLink` behind them. All data
//! is authored synthetic fixture data; nothing here says how the original's
//! multiplayer screens were laid out.

use std::collections::BTreeSet;
use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};

use cs_app::campaign::lower_campaign;
use cs_app::ui::front_end::{
    Action, FlowError, FlowSetup, FrontEnd, FrontEndFlow, LobbyInputs, LobbyStep, Refusal,
    RequestKind, Screen, ScreenDeck, ScreenSessionError, find, validate_table,
};
use cs_app::ui::lobby::{
    Field, HostDraft, HostSettings, JoinDraft, LobbyLink, LobbyUiError, Stage,
};
use cs_content::campaign::declared_synthetic_campaign;
use cs_net::fixture::{SYNTHETIC_SESSION, synthetic_hello, synthetic_parameters};
use cs_net::lobby::{
    AccessPolicy, Admission, Callsign, Departure, HostLoss, JoinError, JoinRequest, LateJoin,
    Loadout, LoadoutProblem, LoadoutValidator, LobbyError, LobbyRules, Phase, Secret, TeamMode,
};
use cs_sim::campaign::{CampaignRunId, DifficultyId};
use cs_types::content::{ContentId, ContentKind};
use cs_types::evidence::ContentHash;

use super::screens::{SURFACE, authored, button_point};

/// The shared loadout validator: `cs_net` cannot depend on the content
/// crates and no production implementation exists yet (#764), so the flow
/// *requires* one to be supplied instead of assuming a permissive default.
struct Accept;
impl LoadoutValidator for Accept {
    fn validate(&self, _: &Loadout) -> Result<(), &'static str> {
        Ok(())
    }
}

/// A validator that refuses everything, with its own stable code — the
/// proof that the injected validator is what the lobby judges by.
struct Reject;
impl LoadoutValidator for Reject {
    fn validate(&self, _: &Loadout) -> Result<(), &'static str> {
        Err("reject-all")
    }
}

fn id(kind: ContentKind, key: &str) -> ContentId {
    ContentId::from_source(kind, key).expect("a valid content id")
}

fn scenario() -> ContentId {
    id(ContentKind::MultiplayerScenario, "synthetic_duel")
}

fn loadout() -> Loadout {
    Loadout {
        blueprint: id(ContentKind::Blueprint, "synthetic_zephyr"),
        components: Vec::new(),
    }
}

fn inputs() -> LobbyInputs {
    LobbyInputs {
        session: SYNTHETIC_SESSION,
        params: synthetic_parameters(),
        hello: synthetic_hello(),
        loadout_validator: Arc::new(|| Box::new(Accept)),
    }
}

fn setup() -> FlowSetup {
    FlowSetup::new(
        std::env::temp_dir().join("cs-f55-cu1-no-profile-is-opened"),
        lower_campaign(&declared_synthetic_campaign()).expect("the fixture campaign lowers"),
        CampaignRunId::new("run.f55cu1").expect("a valid run id"),
        DifficultyId::new("standard").expect("a valid difficulty"),
    )
    .sandbox()
}

/// A deck carrying the install screen, the menu and the lobby screen —
/// authored fixture art, like every deck in this file — so the screens are
/// reached and left by clicking authored buttons.
fn lobby_deck() -> ScreenDeck {
    ScreenDeck::new(vec![
        (Screen::InstallSelect, authored("art-install", &["quit"])),
        (
            Screen::MainMenu,
            authored(
                "art-main-menu",
                &[
                    "open-campaign",
                    "open-instant-action",
                    "open-lobby",
                    "open-settings",
                    "open-credits",
                    "quit",
                ],
            ),
        ),
        (Screen::Lobby, authored("art-lobby", &["back"])),
    ])
    .expect("the lobby deck validates against the state table")
}

/// A flow on the main menu with the lobby inputs supplied.
fn at_menu() -> FrontEndFlow {
    let mut flow = FrontEndFlow::new(lobby_deck(), setup().lobby(inputs()));
    flow.press(Action::InstallVerified)
        .expect("the install verifies");
    assert_eq!(flow.front_end().screen(), Screen::MainMenu);
    flow
}

/// A flow standing on the lobby screen.
fn at_lobby() -> FrontEndFlow {
    let mut flow = at_menu();
    flow.press(Action::OpenLobby).expect("the lobby opens");
    assert_eq!(flow.front_end().screen(), Screen::Lobby);
    flow
}

/// Clicks an authored button under its own centre.
fn click(flow: &mut FrontEndFlow, action: Action) {
    let (x, y) = button_point(flow.session(), action);
    flow.click(SURFACE, x, y).unwrap_or_else(|why| {
        panic!(
            "clicking {action:?} on {:?}: {why}",
            flow.front_end().screen()
        )
    });
}

const TARGET: &str = "192.0.2.10:7777";

/// Fills and submits the host form through the flow, landing in the room.
fn host_through(flow: &mut FrontEndFlow) {
    flow.edit_lobby(|screen| {
        screen.show_host().expect("the host form shows");
        screen.set_scenario(scenario()).expect("a scenario id");
        screen.set_host_callsign("Host").expect("a callsign");
        screen.set_max_members(4).expect("a member cap");
    })
    .expect("on the lobby screen");
    flow.submit_lobby_host()
        .expect("the host form opens a lobby");
    let domain = flow.domain();
    let screen = domain.lobby_screen().expect("a lobby screen");
    assert_eq!(screen.stage(), Stage::Room);
    assert_eq!(
        screen.local(),
        Some(
            flow.domain()
                .lobby_link()
                .expect("a hosted link")
                .host_peer()
        )
    );
}

/// The resolved endpoint a join is submitted against: another part of the
/// application opened this lobby (`LobbyLink::host`), exactly what
/// resolving the join form's direct address produces today.
fn endpoint_lobby(password: Option<&str>, max_members: usize) -> LobbyLink {
    LobbyLink::host(
        HostSettings {
            session: SYNTHETIC_SESSION,
            params: synthetic_parameters(),
            admission: Admission {
                access: password.map_or(AccessPolicy::Open, |text| {
                    AccessPolicy::Password(Secret::new(text).expect("a valid secret"))
                }),
                max_members,
                host_loss: HostLoss::EndSession,
            },
            rules: LobbyRules {
                scenario: scenario(),
                banned: BTreeSet::new(),
                team_mode: TeamMode::FreeForAll,
                late_join: LateJoin::Closed,
            },
            host_callsign: Callsign::new("Host").expect("a valid callsign"),
        },
        Box::new(Accept),
    )
    .expect("the endpoint lobby opens")
}

/// **The task's acceptance path:** the lobby is a screen of the F45-A
/// state table — entered from the main menu through `FrontEndFlow::click`
/// on the authored `open-lobby` button, left by `Back`, which on a join or
/// host form is `LobbyScreen::back`'s `Back::Exit`: the drafts are
/// discarded and the machine lands on the menu. Removing either row breaks
/// this test.
#[test]
fn accept_f55_cu1_the_lobby_is_reached_from_the_main_menu_and_back_discards_the_drafts() {
    let mut flow = at_menu();

    click(&mut flow, Action::OpenLobby);
    assert_eq!(flow.front_end().screen(), Screen::Lobby);
    // Entering resets focus and transient state — the machine's button
    // focus on the lobby's own escape, and the screen on its first field.
    assert_eq!(flow.front_end().focus(), Some(Action::Back));
    {
        let domain = flow.domain();
        let screen = domain.lobby_screen().expect("a lobby screen is open");
        assert_eq!(screen.stage(), Stage::Join);
        assert_eq!(screen.focus(), Field::Target);
        assert_eq!(screen.join_draft(), &JoinDraft::default());
        assert_eq!(screen.host_draft(), &HostDraft::default());
        assert_eq!(screen.refusal(), None);
    }

    // Fill both forms' drafts and leave a kept refusal on the screen.
    flow.edit_lobby(|screen| {
        screen.set_target(TARGET).expect("a direct address");
        screen.set_callsign("Ace").expect("a callsign");
        screen.show_host().expect("the host form shows");
        screen.set_host_callsign("Host").expect("a callsign");
        screen.set_max_members(4).expect("a member cap");
        screen
            .set_target(&"a".repeat(200))
            .expect_err("an overlong entry is refused and kept");
    })
    .expect("on the lobby screen");
    assert!(
        flow.domain().lobby_screen().unwrap().refusal().is_some(),
        "the screen keeps the refusal for display"
    );

    // Back on the form is `Back::Exit`: the screen and its link are
    // dropped and the machine is on the menu.
    let step = flow.lobby_back().expect("Back leaves the lobby");
    assert!(matches!(step, LobbyStep::Exited(_)), "{step:?}");
    assert_eq!(flow.front_end().screen(), Screen::MainMenu);
    assert!(flow.domain().lobby_screen().is_none());
    assert!(flow.domain().lobby_link().is_none());

    // Re-entering is a fresh screen: nothing of the last visit's drafts,
    // focus or refusal survives.
    click(&mut flow, Action::OpenLobby);
    let domain = flow.domain();
    let screen = domain.lobby_screen().expect("the lobby reopened");
    assert_eq!(screen.stage(), Stage::Join);
    assert_eq!(screen.focus(), Field::Target);
    assert_eq!(screen.join_draft(), &JoinDraft::default());
    assert_eq!(screen.host_draft(), &HostDraft::default());
    assert_eq!(screen.refusal(), None);

    // And a click on the authored `back` button is the same exit.
    click(&mut flow, Action::Back);
    assert_eq!(flow.front_end().screen(), Screen::MainMenu);
    assert!(flow.domain().lobby_screen().is_none());
}

/// `Back` inside the room follows the lobby's own stage table — leave the
/// room first — and only then is the front-end row's `Back::Exit` taken.
#[test]
fn accept_f55_cu1_back_inside_the_room_leaves_the_room_first_then_exits() {
    let mut flow = at_lobby();
    host_through(&mut flow);
    assert_eq!(
        flow.domain().lobby_link().unwrap().lobby().phase(),
        Phase::Gathering
    );

    // A raw Back press is refused while the room is open: the front-end
    // row must not drop a live room behind the screen's back.
    let refused = flow.press(Action::Back).unwrap_err();
    assert!(
        matches!(refused, FlowError::LobbyRoomStillOpen),
        "{refused:?}"
    );
    assert_eq!(flow.front_end().screen(), Screen::Lobby);
    assert_eq!(flow.domain().lobby_screen().unwrap().stage(), Stage::Room);

    // The flow's Back performs the lobby's own step: the host leaving
    // closes the lobby (the declared host-loss policy) and the screen
    // lands on a fresh join form, still on the lobby screen.
    let step = flow.lobby_back().expect("Back leaves the room first");
    assert!(
        matches!(step, LobbyStep::LeftRoom(Some(Departure::LobbyClosed))),
        "{step:?}"
    );
    assert_eq!(flow.front_end().screen(), Screen::Lobby);
    {
        let domain = flow.domain();
        let screen = domain.lobby_screen().unwrap();
        assert_eq!(screen.stage(), Stage::Join);
        assert_eq!(screen.local(), None);
    }
    assert_eq!(
        flow.domain().lobby_link().unwrap().lobby().phase(),
        Phase::Closed
    );

    // Now Back is the form's `Back::Exit` and the machine leaves.
    let step = flow.lobby_back().expect("Back exits for the menu");
    assert!(matches!(step, LobbyStep::Exited(_)), "{step:?}");
    assert_eq!(flow.front_end().screen(), Screen::MainMenu);
    assert!(flow.domain().lobby_screen().is_none());
    assert!(flow.domain().lobby_link().is_none());
}

/// While the room is open the live link is not swapped out from under
/// it: a resolved join endpoint arriving then is refused, the hosted
/// lobby stays the room's link, and the departure a later `Back`
/// broadcasts is still its own. On the join form the same call is the
/// endpoint's job and stands.
#[test]
fn accept_f55_cu1_the_rooms_live_link_is_not_swapped_while_open() {
    let mut flow = at_lobby();
    host_through(&mut flow);
    let host = flow
        .domain()
        .lobby_link()
        .expect("a hosted link")
        .host_peer();

    let refused = flow
        .set_lobby_endpoint(endpoint_lobby(None, 4))
        .unwrap_err();
    assert!(
        matches!(refused, FlowError::LobbyRoomStillOpen),
        "{refused:?}"
    );
    {
        let domain = flow.domain();
        let link = domain.lobby_link().expect("the live link is kept");
        assert_eq!(link.host_peer(), host);
        assert_eq!(link.lobby().phase(), Phase::Gathering);
    }

    // Back still tears the hosted room down and reports its departure —
    // the leave runs on the lobby the player is in, not the refused
    // endpoint's.
    let step = flow.lobby_back().expect("Back leaves the room");
    assert!(
        matches!(step, LobbyStep::LeftRoom(Some(Departure::LobbyClosed))),
        "{step:?}"
    );
    assert_eq!(
        flow.domain().lobby_link().unwrap().lobby().phase(),
        Phase::Closed
    );

    // And on the join form the resolved endpoint is the link again.
    flow.set_lobby_endpoint(endpoint_lobby(None, 4))
        .expect("the join form takes a resolved endpoint");
}

/// The application supplies `submit_host`'s outside values through
/// `LobbyInputs`: the hosted lobby is opened under the supplied session
/// epoch and compatibility signature, and the supplied validator factory
/// builds the shared `LoadoutValidator` the room's loadouts are judged by.
#[test]
fn accept_f55_cu1_submit_host_uses_the_supplied_epoch_signature_and_validator() {
    let built = Arc::new(AtomicUsize::new(0));
    let counting = built.clone();
    let mut inputs = inputs();
    inputs.loadout_validator = Arc::new(move || {
        counting.fetch_add(1, Ordering::SeqCst);
        Box::new(Reject)
    });
    let mut flow = FrontEndFlow::new(lobby_deck(), setup().lobby(inputs));
    flow.press(Action::InstallVerified)
        .expect("the install verifies");
    flow.press(Action::OpenLobby).expect("the lobby opens");
    host_through(&mut flow);
    assert_eq!(
        built.load(Ordering::SeqCst),
        1,
        "the supplied factory built the validator"
    );

    // The epoch is the supplied one...
    assert_eq!(
        flow.domain()
            .lobby_link()
            .expect("a hosted link")
            .lobby()
            .session(),
        SYNTHETIC_SESSION
    );

    // ...the compatibility signature is the supplied one: a matching
    // hello is admitted and a foreign one is refused by content, on the
    // very lobby the form opened.
    let callsign = || Callsign::new("Ace").expect("a callsign");
    let admitted = flow
        .lobby_action(|_screen, link| {
            link.join(JoinRequest {
                hello: synthetic_hello(),
                callsign: callsign(),
                password: None,
            })
        })
        .expect("in the room")
        .expect("the supplied signature admits a matching client")
        .0;
    let mut foreign = synthetic_hello();
    foreign.compatibility.content_sha256 = ContentHash::from_bytes([1; 32]);
    let mismatched = flow
        .lobby_action(|_screen, link| {
            link.join(JoinRequest {
                hello: foreign,
                callsign: callsign(),
                password: None,
            })
        })
        .expect("in the room")
        .unwrap_err();
    assert!(matches!(mismatched, JoinError::ContentMismatch(_)));
    assert_eq!(
        flow.domain()
            .lobby_link()
            .unwrap()
            .lobby()
            .members()
            .count(),
        2,
        "the matching join took a seat"
    );
    assert!(admitted.get() > 0);

    // ...and the validator the factory built is what the lobby judges
    // loadouts by: `reject-all` is its own stable code.
    let refused = flow
        .lobby_action(|screen, link| screen.set_loadout(link, loadout()))
        .expect("in the room")
        .unwrap_err();
    assert_eq!(
        refused,
        LobbyUiError::Domain(LobbyError::LoadoutRejected(LoadoutProblem::Invalid {
            code: "reject-all"
        }))
    );
}

/// The application supplies `submit_join`'s outside values: the resolved
/// endpoint through `set_lobby_endpoint` and the client hello through
/// `LobbyInputs`. A refusal is the host's distinct reason and the endpoint
/// stays for the corrected retry.
#[test]
fn accept_f55_cu1_submit_join_uses_the_resolved_endpoint_and_the_supplied_hello() {
    let mut flow = at_lobby();
    // Without a resolved endpoint the submit is a wiring refusal, not a
    // join error.
    assert!(matches!(
        flow.submit_lobby_join().unwrap_err(),
        FlowError::NoLobbyLink
    ));

    flow.set_lobby_endpoint(endpoint_lobby(Some("hunter2"), 2))
        .expect("the endpoint is supplied");
    flow.edit_lobby(|screen| {
        screen.set_target(TARGET).expect("a direct address");
        screen.set_callsign("Ace").expect("a callsign");
        screen.set_password("nope").expect("a password");
    })
    .expect("on the lobby screen");

    // The wrong password is refused by name and the form stays for a
    // retry: draft, stage and endpoint untouched.
    let refused = flow.submit_lobby_join().unwrap_err();
    assert!(
        matches!(
            refused,
            FlowError::Lobby(LobbyUiError::Join(JoinError::WrongPassword))
        ),
        "{refused:?}"
    );
    {
        let domain = flow.domain();
        let screen = domain.lobby_screen().unwrap();
        assert_eq!(screen.stage(), Stage::Join);
        assert_eq!(screen.join_draft().password, "nope");
        assert_eq!(
            screen.refusal().map(|error| error.message_key()),
            Some("lobby.join.wrong_password")
        );
    }
    assert_eq!(
        flow.domain()
            .lobby_link()
            .unwrap()
            .lobby()
            .members()
            .count(),
        1,
        "a refused join allocates no member"
    );

    // The corrected draft joins on the same endpoint.
    flow.edit_lobby(|screen| screen.set_password("hunter2"))
        .expect("on the lobby screen")
        .expect("a corrected entry");
    let peer = flow.submit_lobby_join().expect("the retry joins");
    {
        let domain = flow.domain();
        let screen = domain.lobby_screen().unwrap();
        assert_eq!(screen.stage(), Stage::Room);
        assert_eq!(screen.local(), Some(peer));
        assert_eq!(screen.refusal(), None);
    }
    assert_eq!(
        flow.domain()
            .lobby_link()
            .unwrap()
            .lobby()
            .members()
            .count(),
        2
    );

    // And the hello the join offered is the supplied one: a lobby visit
    // wired with a foreign signature is refused by content, not by
    // password.
    let mut foreign_inputs = inputs();
    foreign_inputs.hello.compatibility.content_sha256 = ContentHash::from_bytes([1; 32]);
    let mut foreign_flow = FrontEndFlow::new(lobby_deck(), setup().lobby(foreign_inputs));
    foreign_flow
        .press(Action::InstallVerified)
        .expect("the install verifies");
    foreign_flow
        .press(Action::OpenLobby)
        .expect("the lobby opens");
    foreign_flow
        .set_lobby_endpoint(endpoint_lobby(Some("hunter2"), 4))
        .expect("the endpoint is supplied");
    foreign_flow
        .edit_lobby(|screen| {
            screen.set_target(TARGET).expect("a direct address");
            screen.set_callsign("Ace").expect("a callsign");
            screen.set_password("hunter2").expect("the right password");
        })
        .expect("on the lobby screen");
    let mismatched = foreign_flow.submit_lobby_join().unwrap_err();
    assert!(matches!(
        mismatched,
        FlowError::Lobby(LobbyUiError::Join(JoinError::ContentMismatch(_)))
    ));
}

/// Entering without the application's lobby inputs is refused by name and
/// nothing moves — a lobby that can never submit is never opened.
#[test]
fn accept_f55_cu1_opening_without_lobby_inputs_is_refused_and_nothing_moves() {
    let mut flow = FrontEndFlow::new(lobby_deck(), setup());
    flow.press(Action::InstallVerified)
        .expect("the install verifies");
    let refused = flow.press(Action::OpenLobby).unwrap_err();
    assert!(matches!(refused, FlowError::NoLobbyInputs), "{refused:?}");
    assert_eq!(flow.front_end().screen(), Screen::MainMenu);
    assert!(flow.domain().lobby_screen().is_none());
    assert!(flow.domain().lobby_link().is_none());
}

/// The two rows the table gained: the menu reaches the lobby, the lobby's
/// only front-end row is its Back, and the bare machine walks them — the
/// domain is what the requests reach, not what moving needs.
#[test]
fn accept_f55_cu1_the_table_declares_the_lobby_rows_and_the_bare_machine_walks_them() {
    assert!(Screen::ALL.contains(&Screen::Lobby));
    let entry = find(Screen::MainMenu, Action::OpenLobby).expect("the menu has a lobby row");
    assert_eq!(entry.to, Screen::Lobby);
    assert_eq!(entry.request, RequestKind::OpenLobby);
    let escape = find(Screen::Lobby, Action::Back).expect("the lobby has a back row");
    assert_eq!(escape.to, Screen::MainMenu);
    assert_eq!(escape.request, RequestKind::LobbyBack);
    assert!(
        validate_table().is_empty(),
        "the table stays total with the lobby on it"
    );

    // The bare machine moves on both rows and asks for the domain
    // requests they declare.
    let mut front_end = FrontEnd::new();
    front_end
        .apply(Action::InstallVerified)
        .expect("the install verifies");
    let plan = front_end.plan(Action::OpenLobby).expect("the entry plans");
    assert_eq!(plan.to, Screen::Lobby);
    assert_eq!(
        plan.request,
        Some(cs_app::ui::front_end::Request::OpenLobby)
    );
    front_end.apply(Action::OpenLobby).expect("the lobby opens");
    assert_eq!(front_end.screen(), Screen::Lobby);
    let plan = front_end.plan(Action::Back).expect("the escape plans");
    assert_eq!(plan.to, Screen::MainMenu);
    assert_eq!(
        plan.request,
        Some(cs_app::ui::front_end::Request::LobbyBack)
    );
    front_end.apply(Action::Back).expect("Back leaves");
    assert_eq!(front_end.screen(), Screen::MainMenu);
}

/// The lobby steps belong to the lobby screen: off it they are refused by
/// name, exactly like every other screen's domain-side operations.
#[test]
fn accept_f55_cu1_lobby_operations_are_refused_off_the_lobby_screen() {
    let mut flow = at_menu();
    let wrong_screen = |error: &FlowError| {
        matches!(
            error,
            FlowError::Screen(ScreenSessionError::Refused(Refusal::WrongScreen {
                expected: Screen::Lobby,
                actual: Screen::MainMenu,
            }))
        )
    };
    let error = flow.edit_lobby(|_| ()).unwrap_err();
    assert!(wrong_screen(&error), "{error:?}");
    let error = flow.lobby_action(|_, _| ()).unwrap_err();
    assert!(wrong_screen(&error), "{error:?}");
    let error = flow.submit_lobby_host().unwrap_err();
    assert!(wrong_screen(&error), "{error:?}");
    let error = flow.submit_lobby_join().unwrap_err();
    assert!(wrong_screen(&error), "{error:?}");
    let error = flow
        .set_lobby_endpoint(endpoint_lobby(None, 4))
        .unwrap_err();
    assert!(wrong_screen(&error), "{error:?}");
    let error = flow.lobby_back().unwrap_err();
    assert!(wrong_screen(&error), "{error:?}");

    // And on the screen with nothing to drive yet, the room action and
    // the join submit name the missing link.
    let mut flow = at_lobby();
    let error = flow.lobby_action(|_, _| ()).unwrap_err();
    assert!(matches!(error, FlowError::NoLobbyLink), "{error:?}");
    let error = flow.submit_lobby_join().unwrap_err();
    assert!(matches!(error, FlowError::NoLobbyLink), "{error:?}");
    assert_eq!(flow.front_end().screen(), Screen::Lobby);
}

/// The navigation review walks the lobby rows like every other: the
/// screen is reached from the menu and left for it, both rows ask the
/// domain for a transaction, and neither is refused — the lobby rows need
/// no domain input the inputless walk could not supply.
#[test]
fn accept_f55_cu1_the_navigation_review_reaches_and_leaves_the_lobby() {
    let review = cs_app::ui::front_end::review_navigation(Screen::START);
    assert!(review.is_complete(), "{review}");
    assert!(review.reached.contains(&Screen::Lobby), "{review}");
    let requests = review.requests();
    assert!(requests.contains(&(Screen::MainMenu, Action::OpenLobby, RequestKind::OpenLobby)));
    assert!(requests.contains(&(Screen::Lobby, Action::Back, RequestKind::LobbyBack)));
    assert!(
        !review
            .refusals()
            .iter()
            .any(|(from, action, _)| *from == Screen::Lobby
                || (*from == Screen::MainMenu && *action == Action::OpenLobby)),
        "no lobby row is refused: {:?}",
        review.refusals()
    );
}
