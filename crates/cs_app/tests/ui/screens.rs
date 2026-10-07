//! Acceptance stage F45-B: the original-asset menu, cabin and briefing
//! screens (`specs/F45-main-menu-pandora-cabin-briefing-and-flight-check.md`,
//! section `### F45-B`).
//!
//! The minimum scenario is AC02 — *cancel at every preflight screen and verify
//! state/currency unchanged* — driven entirely through the authored artwork
//! (`ScreenSession::click` on the fitted screen), plus the stage's
//! non-negotiable behaviour: every authored hotspot is a working button,
//! focus visits the authored order, replaying the briefing and looking at
//! recon never start the mission, a screen without authored assets is
//! reported rather than placeholdered, and cancelling a load keeps the
//! selection and releases the world.
//!
//! The artwork, hotspot rectangles and their order here are authored fixture
//! data (`art-*` images with `ui-resource` buttons): **no original screen,
//! hotspot coordinate or artwork id is decoded anywhere in this repository**
//! (`ui_resource` and `image` hold no rows in the retail baseline — see
//! `docs/findings/2026-10-07-f45-b-original-asset-screen-decks.md`), so the
//! stage proves the deck, the presentation and their refusal rules, never the
//! original front end (F45-D captures that, `retail,gpu`). The campaign whose
//! state and currency must not move is a real `cs_sim` state with one victory
//! already applied.

use cs_app::campaign::lower_campaign;
use cs_app::ui::front_end::{
    Action, ConstructionDraft, DeckError, Effect, LayoutProblem, Loadout, ProfileIntent, Refusal,
    Request, Resource, Screen, ScreenAssetError, ScreenAssets, ScreenDeck, ScreenSession,
    ScreenSessionError,
};
use cs_content::campaign::declared_synthetic_campaign;
use cs_content::ui_layout::{Hotspot, LayoutError, Rect, ScreenLayout};
use cs_sim::campaign::{
    CampaignNodeKey, CampaignRunId, CampaignState, DifficultyId, EventKey, MissionOutcome,
    Outcome as MissionResult, OutcomeAuthority, OutcomeId, ProfileId, SessionGeneration, SymbolId,
};
use cs_types::Tick;
use cs_types::content::{ContentId, ContentKind};

use super::id;

/// The logical size of every authored image in this file.
const IMAGE: (u32, u32) = (640, 480);
/// The surface every click and view is made against.
const SURFACE: (u32, u32) = (1920, 1080);

const fn rect(x: u32, y: u32, width: u32, height: u32) -> Rect {
    Rect {
        x,
        y,
        width,
        height,
    }
}

fn hotspot(key: &str, action: &str, rect: Rect) -> Hotspot {
    Hotspot {
        id: id(ContentKind::UiResource, key),
        action: action.to_owned(),
        rect,
    }
}

/// The authored art of one screen: `key` names the artwork, `actions` are the
/// button keys in the order focus visits them, stacked down the image.
fn authored(key: &str, actions: &[&str]) -> ScreenAssets {
    let hotspots = actions
        .iter()
        .enumerate()
        .map(|(index, action)| {
            hotspot(
                &format!("{key}.{action}"),
                action,
                rect(40, 40 + 60 * index as u32, 560, 50),
            )
        })
        .collect();
    let layout =
        ScreenLayout::new(IMAGE, hotspots).expect("the authored buttons are inside the art");
    ScreenAssets::new(id(ContentKind::Image, key), &layout).expect("the artwork is an image")
}

/// A deck carrying every screen a player reaches before flying, with the
/// briefing's authored order deliberately different from the table's.
fn preflight_deck() -> ScreenDeck {
    ScreenDeck::new(vec![
        (Screen::InstallSelect, authored("art-install", &["quit"])),
        (
            Screen::ContentDiagnosis,
            authored("art-diagnosis", &["choose-another-install", "quit"]),
        ),
        (
            Screen::MainMenu,
            authored(
                "art-main-menu",
                &["new-profile", "continue-profile", "open-settings", "quit"],
            ),
        ),
        (
            Screen::ProfileSelect,
            authored("art-profile-select", &["confirm-profile", "back"]),
        ),
        (Screen::Settings, authored("art-settings", &["back"])),
        (
            Screen::Cabin,
            authored(
                "art-cabin",
                &[
                    "open-briefing",
                    "open-scrapbook",
                    "open-construction",
                    "back",
                ],
            ),
        ),
        (Screen::Scrapbook, authored("art-scrapbook", &["back"])),
        (
            Screen::Briefing,
            authored(
                "art-briefing",
                &[
                    "back",
                    "continue-to-flight-check",
                    "open-recon",
                    "replay-briefing",
                ],
            ),
        ),
        (Screen::Recon, authored("art-recon", &["back"])),
        (
            Screen::Construction,
            authored(
                "art-construction",
                &["back", "cancel", "commit-construction"],
            ),
        ),
        (
            Screen::FlightCheck,
            authored("art-flight-check", &["back", "cancel", "launch"]),
        ),
        (Screen::Loading, authored("art-loading", &["cancel"])),
    ])
    .expect("the preflight deck validates against the state table")
}

fn press(session: &mut ScreenSession, action: Action) -> Vec<Effect> {
    session
        .press(action)
        .unwrap_or_else(|why| panic!("{action:?} on {:?}: {why}", session.front_end().screen()))
        .effects
}

/// The surface point at the centre of a screen's authored button.
fn button_point(session: &ScreenSession, action: Action) -> (u32, u32) {
    let view = session.view(SURFACE).expect("the screen is authored");
    let button = view
        .buttons
        .iter()
        .find(|button| button.action == action)
        .unwrap_or_else(|| panic!("{action:?} is not authored on {:?}", view.screen));
    (
        button.rect.x + button.rect.width / 2,
        button.rect.y + button.rect.height / 2,
    )
}

/// Presses the authored button under its own centre.
fn click(session: &mut ScreenSession, action: Action) -> Vec<Effect> {
    let (x, y) = button_point(session, action);
    session
        .click(SURFACE, x, y)
        .unwrap_or_else(|why| {
            panic!(
                "clicking {action:?} on {:?}: {why}",
                session.front_end().screen()
            )
        })
        .effects
}

fn requests(effects: &[Effect]) -> Vec<Request> {
    effects
        .iter()
        .filter_map(|effect| match effect {
            Effect::Request(request) => Some(request.clone()),
            _ => None,
        })
        .collect()
}

/// A real campaign with one victory already applied: currency is 500, not a
/// zero that an accidental write could imitate.
fn campaign() -> CampaignState {
    let graph = lower_campaign(&declared_synthetic_campaign()).expect("the fixture lowers");
    let mut state = CampaignState::begin(
        ProfileId::new("pilot.nathan").expect("valid"),
        CampaignRunId::new("run.one").expect("valid"),
        DifficultyId::new("standard").expect("valid"),
        &graph,
    );
    let victory = MissionOutcome {
        id: OutcomeId {
            profile: ProfileId::new("pilot.nathan").expect("valid"),
            run: CampaignRunId::new("run.one").expect("valid"),
            session: SessionGeneration(1),
            terminal_event: EventKey {
                session: SessionGeneration(1),
                tick: Tick(480),
                source: SymbolId(7),
                sequence: 0,
            },
        },
        node: CampaignNodeKey::new("m01").expect("valid node key"),
        outcome: MissionResult::Succeeded,
        score: 900,
        authority: OutcomeAuthority::Authorized,
    };
    state
        .apply_outcome(&graph, &victory)
        .expect("the first application commits");
    assert_eq!(state.currency(), 500, "the fixture won its first mission");
    state
}

fn good_loadout() -> Loadout {
    Loadout {
        player: Some(id(ContentKind::Airframe, "player-plane")),
        wingmate: Some(id(ContentKind::Airframe, "wing-plane")),
        ammunition: Some(id(ContentKind::Ammo, "standard")),
    }
}

fn to_cabin(session: &mut ScreenSession) -> Vec<Request> {
    let mut made = requests(&press(session, Action::InstallVerified));
    made.extend(requests(&click(session, Action::NewProfile)));
    made.extend(requests(&click(session, Action::ConfirmProfile)));
    assert_eq!(session.front_end().screen(), Screen::Cabin);
    made
}

/// **AC02 — the stage's minimum scenario.** Cancel (or Back, or Quit where a
/// screen has nothing else to leave by) at **every** preflight screen — all
/// twelve of them, the whole `preflight` list the deck must carry — driven
/// through the authored buttons, and the campaign's state and currency are
/// unchanged: no cancel path produces a transaction the domain could apply to
/// the campaign (applying one is F45-C's wiring; this stage's half of that
/// contract is that cancelling has nothing to apply). The single transaction
/// a cancel asks for anywhere in the walk is leaving the cabin, which closes
/// the profile — the profile's own save/close, never a campaign one.
#[test]
fn accept_f45_b_cancel_at_every_preflight_screen_leaves_state_and_currency_unchanged() {
    let campaign = campaign();
    let before = campaign.snapshot();

    // Every screen a player reaches before flying carries authored art; the
    // in-flight and results screens are F45-B's later stages.
    let deck = preflight_deck();
    let preflight: Vec<Screen> = Screen::ALL
        .into_iter()
        .filter(|screen| {
            !matches!(
                screen,
                Screen::Flight | Screen::Pause | Screen::PauseSettings | Screen::Results
            )
        })
        .collect();
    assert_eq!(deck.screens().collect::<Vec<_>>(), preflight);

    let mut cancelled: Vec<Request> = Vec::new();
    // Every screen this walk actually cancels at, so "every preflight screen"
    // below is an assertion and not a claim in a comment. Loading's own
    // cancel lives in `accept_f45_b_cancelling_the_load_…`, which also pins
    // the campaign snapshot.
    let mut cancelled_at: Vec<Screen> = Vec::new();

    // The menu screens: reaching them and backing out is not a transaction.
    for (open, screen) in [
        (Action::NewProfile, Screen::ProfileSelect),
        (Action::OpenSettings, Screen::Settings),
    ] {
        let mut session = ScreenSession::new(deck.clone());
        press(&mut session, Action::InstallVerified);
        assert_eq!(
            requests(&click(&mut session, open)),
            Vec::<Request>::new(),
            "reaching {screen:?} is not a transaction"
        );
        assert_eq!(session.front_end().screen(), screen);
        let leave = click(&mut session, Action::Back);
        assert_eq!(session.front_end().screen(), Screen::MainMenu);
        assert!(
            requests(&leave).is_empty(),
            "cancel on {screen:?} asked the domain for something"
        );
        cancelled.extend(requests(&leave));
        cancelled_at.push(screen);
    }

    // The cabin and its screens. Opening the profile is the one transaction
    // this walk performs; every cancel after it must ask for nothing.
    let cabin_walks: [(Screen, &[Action], Action, Screen, bool); 5] = [
        (
            Screen::Briefing,
            &[Action::OpenBriefing],
            Action::Back,
            Screen::Cabin,
            false,
        ),
        (
            Screen::Recon,
            &[Action::OpenBriefing, Action::OpenRecon],
            Action::Back,
            Screen::Briefing,
            false,
        ),
        (
            Screen::Scrapbook,
            &[Action::OpenScrapbook],
            Action::Back,
            Screen::Cabin,
            false,
        ),
        (
            Screen::Construction,
            &[Action::OpenConstruction],
            Action::Cancel,
            Screen::Cabin,
            true,
        ),
        (
            Screen::FlightCheck,
            &[Action::OpenBriefing, Action::ContinueToFlightCheck],
            Action::Cancel,
            Screen::Briefing,
            true,
        ),
    ];
    for (screen, steps, leave, leave_to, dirty) in cabin_walks {
        let mut session = ScreenSession::new(deck.clone());
        let mut reach = to_cabin(&mut session);
        for step in steps {
            reach.extend(requests(&click(&mut session, *step)));
        }
        assert_eq!(session.front_end().screen(), screen);
        assert_eq!(
            reach,
            vec![Request::OpenProfile(ProfileIntent::New)],
            "reaching {screen:?} asks for nothing beyond the profile"
        );

        if dirty {
            if screen == Screen::Construction {
                session
                    .open_construction(ConstructionDraft {
                        blueprint: id(ContentKind::Blueprint, "custom"),
                        saved: vec![],
                        components: vec![],
                    })
                    .expect("open");
                session
                    .edit_construction(vec![id(ContentKind::HardpointEquipment, "gun-mount")])
                    .expect("edit");
                assert!(
                    session
                        .front_end()
                        .construction()
                        .expect("draft")
                        .is_dirty(),
                    "the draft really is dirty"
                );
            } else {
                session.select_loadout(good_loadout()).expect("select");
            }
            // A dirty draft asks first: the prompt itself is no transaction.
            let prompt = click(&mut session, leave);
            assert_eq!(prompt, vec![Effect::AskDiscard], "{screen:?}");
            assert_eq!(session.front_end().screen(), screen);
            // While the prompt is open nothing slips past it: pressing the
            // same authored cancel again and moving focus are both refused,
            // and a refusal leaves the machine exactly as it was.
            let (x, y) = button_point(&session, leave);
            assert_eq!(
                session.click(SURFACE, x, y),
                Err(ScreenSessionError::Refused(Refusal::ConfirmationPending)),
                "the open prompt refuses another click on {screen:?}"
            );
            assert_eq!(
                session.move_focus(true),
                Err(ScreenSessionError::Refused(Refusal::ConfirmationPending)),
                "the open prompt refuses focus movement on {screen:?}"
            );
            assert_eq!(session.front_end().screen(), screen);
            let confirmed = session.confirm_discard().expect("discard confirmed");
            let leave_effects = confirmed.effects;
            assert_eq!(confirmed.to, leave_to, "{screen:?}");
            if screen == Screen::Construction {
                assert!(
                    session.front_end().construction().is_none(),
                    "the discarded draft is gone"
                );
            } else {
                assert!(
                    session.front_end().loadout().is_empty(),
                    "the discarded selection is gone"
                );
            }
            assert!(
                requests(&leave_effects).is_empty(),
                "discarding on {screen:?} asked the domain for something"
            );
            cancelled.extend(requests(&leave_effects));
        } else {
            let leave_effects = click(&mut session, leave);
            assert_eq!(session.front_end().screen(), leave_to, "{screen:?}");
            assert!(
                requests(&leave_effects).is_empty(),
                "cancel on {screen:?} asked the domain for something"
            );
            cancelled.extend(requests(&leave_effects));
        }
        cancelled_at.push(screen);
    }

    // The four preflight screens the walks above pass through without
    // cancelling *from*: the install selection and the main menu have only
    // Quit to leave by, the diagnosis returns to choosing an installation,
    // and the cabin's own escape closes the profile.
    let mut session = ScreenSession::new(deck.clone());
    let quit = click(&mut session, Action::Quit);
    assert_eq!(session.front_end().screen(), Screen::InstallSelect);
    assert!(
        quit.contains(&Effect::ExitApplication),
        "quitting the install selection leaves the application"
    );
    assert!(
        requests(&quit).is_empty(),
        "cancel on InstallSelect asked the domain for something"
    );
    cancelled_at.push(Screen::InstallSelect);

    let mut session = ScreenSession::new(deck.clone());
    press(&mut session, Action::InstallRejected);
    assert_eq!(session.front_end().screen(), Screen::ContentDiagnosis);
    let again = click(&mut session, Action::ChooseAnotherInstall);
    assert_eq!(session.front_end().screen(), Screen::InstallSelect);
    assert!(
        requests(&again).is_empty(),
        "cancel on ContentDiagnosis asked the domain for something"
    );
    cancelled_at.push(Screen::ContentDiagnosis);

    let mut session = ScreenSession::new(deck.clone());
    press(&mut session, Action::InstallVerified);
    assert_eq!(session.front_end().screen(), Screen::MainMenu);
    let quit = click(&mut session, Action::Quit);
    assert_eq!(session.front_end().screen(), Screen::MainMenu);
    assert!(
        quit.contains(&Effect::ExitApplication),
        "quitting the main menu leaves the application"
    );
    assert!(
        requests(&quit).is_empty(),
        "cancel on MainMenu asked the domain for something"
    );
    cancelled_at.push(Screen::MainMenu);

    // Leaving the cabin is a cancel too, and the one transaction a cancel
    // asks for anywhere in this walk: closing the profile, which is the
    // profile's own save/close and never a campaign one.
    let mut session = ScreenSession::new(deck.clone());
    assert_eq!(
        to_cabin(&mut session),
        vec![Request::OpenProfile(ProfileIntent::New)]
    );
    let leave = click(&mut session, Action::Back);
    assert_eq!(session.front_end().screen(), Screen::MainMenu);
    assert_eq!(
        requests(&leave),
        vec![Request::CloseProfile],
        "leaving the cabin closes the profile and asks for nothing else"
    );
    cancelled_at.push(Screen::Cabin);

    assert!(
        cancelled.is_empty(),
        "a cancel path produced a transaction: {cancelled:?}"
    );
    let mut covered = cancelled_at;
    covered.sort_unstable();
    let expected: Vec<Screen> = preflight
        .iter()
        .copied()
        .filter(|screen| *screen != Screen::Loading)
        .collect();
    assert_eq!(
        covered, expected,
        "every preflight screen except Loading (cancelled in the load test) is cancelled here"
    );
    assert_eq!(
        campaign.snapshot(),
        before,
        "the campaign's state and currency did not move"
    );
    assert_eq!(campaign.currency(), 500);
    assert_eq!(campaign.revision(), before.revision);
}

/// Focus visits the **authored** order of the screen (the table's own row
/// order only stands in when the deck carries no assets), wraps, and activates
/// through the same table the pointer uses.
#[test]
fn accept_f45_b_focus_visits_the_authored_button_order_and_activates_it() {
    let mut session = ScreenSession::new(preflight_deck());
    to_cabin(&mut session);
    click(&mut session, Action::OpenBriefing);

    // The authored briefing order starts with Back; the table's own order
    // starts with ReplayBriefing. A session that walked the table would focus
    // a different button on entry.
    let authored: Vec<Action> = session
        .deck()
        .assets(Screen::Briefing)
        .expect("the briefing is authored")
        .buttons()
        .iter()
        .map(|button| button.action)
        .collect();
    assert_eq!(
        authored,
        vec![
            Action::Back,
            Action::ContinueToFlightCheck,
            Action::OpenRecon,
            Action::ReplayBriefing,
        ]
    );
    assert_ne!(
        authored,
        session.front_end().visible_actions(),
        "the authored order really differs from the table's"
    );

    assert_eq!(
        session.front_end().focus(),
        Some(Action::Back),
        "entering a screen focuses its authored first button"
    );
    let view = session.view(SURFACE).expect("view");
    let image_rect = view.fit.image_rect();
    let focused: Vec<Action> = view
        .buttons
        .iter()
        .filter(|button| button.focused)
        .map(|button| button.action)
        .collect();
    assert_eq!(focused, vec![Action::Back]);
    for button in &view.buttons {
        assert!(
            image_rect.contains(button.rect.x, button.rect.y),
            "{:?} leaves the fitted art",
            button.rect
        );
    }

    session.move_focus(true).expect("focus");
    assert_eq!(
        session.front_end().focus(),
        Some(Action::ContinueToFlightCheck)
    );
    session.move_focus(true).expect("focus");
    assert_eq!(session.front_end().focus(), Some(Action::OpenRecon));

    // The keyboard activates the focused authored button...
    let outcome = session.activate().expect("activate");
    assert_eq!(outcome.from, Screen::Briefing);
    assert_eq!(outcome.to, Screen::Recon);
    // ... and entering recon focuses *its* authored first button.
    assert_eq!(session.front_end().focus(), Some(Action::Back));
    // One button: every direction wraps onto it.
    session.move_focus(true).expect("focus");
    assert_eq!(session.front_end().focus(), Some(Action::Back));
    session.move_focus(false).expect("focus");
    assert_eq!(session.front_end().focus(), Some(Action::Back));

    press(&mut session, Action::Back);
    assert_eq!(session.front_end().focus(), Some(Action::Back));
    session.move_focus(true).expect("focus");
    session.move_focus(true).expect("focus");
    session.move_focus(true).expect("focus");
    session.move_focus(true).expect("focus");
    assert_eq!(
        session.front_end().focus(),
        Some(Action::Back),
        "forward wraps to the authored first button"
    );
    session.move_focus(false).expect("focus");
    assert_eq!(
        session.front_end().focus(),
        Some(Action::ReplayBriefing),
        "backward wraps to the authored last button"
    );
}

/// A click outside the fitted art, in the letterbox bars, hits nothing even
/// where the top-left button *would* be without the fit's offset; a click
/// between two buttons hits nothing either.
#[test]
fn accept_f45_b_a_click_off_the_authored_art_or_between_buttons_hits_nothing() {
    let mut session = ScreenSession::new(preflight_deck());
    press(&mut session, Action::InstallVerified);
    assert_eq!(session.front_end().screen(), Screen::MainMenu);

    // 640x480 art fitted into 1920x1080: 1440x1080 centred, 240px bars.
    let view = session.view(SURFACE).expect("view");
    assert_eq!(view.fit.image_rect(), rect(240, 0, 1440, 1080));

    // The authored first button starts at logical (40, 40): on the surface
    // that is (330, 90). (100, 100) is where it would be *without* the fit's
    // 240px offset, so it sits in the left bar and must hit nothing.
    let first = &view.buttons[0];
    assert_eq!(first.action, Action::NewProfile);
    assert_eq!(first.rect, rect(330, 90, 1260, 112));
    for (x, y) in [(100, 100), (1900, 10), (10, 1000)] {
        assert_eq!(
            session.click(SURFACE, x, y),
            Err(ScreenSessionError::NoButtonAt { x, y }),
            "({x}, {y}) is off the authored art"
        );
    }
    // Inside the art, in the gap between two buttons: still nothing.
    assert_eq!(
        session.click(SURFACE, 500, 210),
        Err(ScreenSessionError::NoButtonAt { x: 500, y: 210 })
    );
    // A hitless click changes nothing.
    assert_eq!(session.front_end().screen(), Screen::MainMenu);
    assert_eq!(session.front_end().focus(), Some(Action::NewProfile));

    // The real button still works through the same transform.
    let (x, y) = button_point(&session, Action::OpenSettings);
    let effects = session.click(SURFACE, x, y).expect("click").effects;
    assert_eq!(session.front_end().screen(), Screen::Settings);
    assert_eq!(requests(&effects), Vec::<Request>::new());

    // A surface nothing can be fitted to is refused, not guessed at.
    assert_eq!(
        session.view((0, 1080)),
        Err(ScreenSessionError::DegenerateSurface { surface: (0, 1080) })
    );
    assert_eq!(
        session.click((0, 1080), 0, 0),
        Err(ScreenSessionError::DegenerateSurface { surface: (0, 1080) })
    );
}

/// The deck refuses the art that would ship a broken front end: a button with
/// no transition, an unknown or application-side key, a screen with no way
/// out, a designed button hidden from navigation, the same screen twice and
/// artwork that is not an image.
#[test]
fn accept_f45_b_the_deck_refuses_a_dead_button_a_hidden_button_and_art_without_escape() {
    // `launch` has no row on the cabin: pressing it would do nothing.
    let dead = ScreenAssets::new(
        id(ContentKind::Image, "art-cabin"),
        &ScreenLayout::new(
            IMAGE,
            vec![hotspot("cabin.launch", "launch", rect(0, 0, 100, 100))],
        )
        .expect("layout"),
    )
    .expect("a known key and a real button");
    assert_eq!(
        ScreenDeck::new(vec![(Screen::Cabin, dead)]),
        Err(DeckError::Layout {
            screen: Screen::Cabin,
            problem: LayoutProblem::NoTransition {
                hotspot: "ui_resource/cabin.launch".to_owned(),
                action: Action::Launch,
            },
        })
    );

    // An unknown key and an application-side result are refused while the
    // hotspots are parsed, before any deck can be built.
    let unknown = ScreenLayout::new(
        IMAGE,
        vec![hotspot("odd", "no-such-action", rect(0, 0, 10, 10))],
    )
    .expect("layout");
    assert_eq!(
        ScreenAssets::new(id(ContentKind::Image, "art-odd"), &unknown),
        Err(ScreenAssetError::Layout {
            problem: LayoutProblem::UnknownAction {
                hotspot: "ui_resource/odd".to_owned(),
                key: "no-such-action".to_owned(),
            },
        })
    );
    let system = ScreenLayout::new(
        IMAGE,
        vec![hotspot("loaded", "load-succeeded", rect(0, 0, 10, 10))],
    )
    .expect("layout");
    assert_eq!(
        ScreenAssets::new(id(ContentKind::Image, "art-loaded"), &system),
        Err(ScreenAssetError::Layout {
            problem: LayoutProblem::NotAButton {
                hotspot: "ui_resource/loaded".to_owned(),
                action: Action::LoadSucceeded,
            },
        })
    );
    // The artwork itself must be an image id.
    assert_eq!(
        ScreenAssets::new(id(ContentKind::UiResource, "art-not-an-image"), &system),
        Err(ScreenAssetError::ArtNotImage {
            art: id(ContentKind::UiResource, "art-not-an-image"),
        })
    );

    // Art with no escape: the screen could not be left, so no deck carries it.
    let trapped = authored(
        "art-briefing",
        &["replay-briefing", "open-recon", "continue-to-flight-check"],
    );
    assert_eq!(
        ScreenDeck::new(vec![(Screen::Briefing, trapped)]),
        Err(DeckError::NoEscapeButton {
            screen: Screen::Briefing
        })
    );

    // Art hiding a designed button: the cabin without its construction door.
    let hidden = authored("art-cabin", &["open-briefing", "open-scrapbook", "back"]);
    assert_eq!(
        ScreenDeck::new(vec![(Screen::Cabin, hidden)]),
        Err(DeckError::HiddenAction {
            screen: Screen::Cabin,
            action: Action::OpenConstruction,
        })
    );

    // The same screen twice is refused rather than silently taking one.
    assert_eq!(
        ScreenDeck::new(vec![
            (Screen::Recon, authored("art-recon", &["back"])),
            (Screen::Recon, authored("art-recon-again", &["back"])),
        ]),
        Err(DeckError::DuplicateScreen {
            screen: Screen::Recon
        })
    );

    // And the complete deck does build.
    assert!(preflight_deck().assets(Screen::Briefing).is_some());
}

/// A screen whose art the loader could not produce is **reported** when it
/// would be drawn — never a placeholder — and stays coherent: focus falls
/// back to the table and the escape still leaves.
#[test]
fn accept_f45_b_a_screen_without_authored_assets_is_reported_never_placeholdered() {
    let deck = ScreenDeck::new(vec![(
        Screen::MainMenu,
        authored(
            "art-main-menu",
            &["new-profile", "continue-profile", "open-settings", "quit"],
        ),
    )])
    .expect("a valid partial deck");
    let mut session = ScreenSession::new(deck);
    press(&mut session, Action::InstallVerified);
    let effects = click(&mut session, Action::OpenSettings);
    assert_eq!(requests(&effects), Vec::<Request>::new());
    assert_eq!(session.front_end().screen(), Screen::Settings);

    assert!(session.deck().assets(Screen::Settings).is_none());
    assert_eq!(
        session.view(SURFACE),
        Err(ScreenSessionError::ScreenNotInDeck {
            screen: Screen::Settings
        })
    );
    assert_eq!(
        session.click(SURFACE, 400, 400),
        Err(ScreenSessionError::ScreenNotInDeck {
            screen: Screen::Settings
        })
    );

    // Focus falls back to the table's own order and the escape works, so an
    // undecorated screen can always be left.
    session.move_focus(false).expect("focus");
    assert_eq!(session.front_end().focus(), Some(Action::Back));
    let outcome = session.press(Action::Back).expect("leave");
    assert_eq!(outcome.to, Screen::MainMenu);
    // Back on the authored menu, focus follows the authored order again.
    assert_eq!(session.front_end().focus(), Some(Action::NewProfile));
}

/// Non-negotiable 2 through the authored screens: replaying the briefing and
/// looking at recon never start the mission and never touch the world.
#[test]
fn accept_f45_b_replaying_the_briefing_and_viewing_recon_never_start_the_mission() {
    let mut session = ScreenSession::new(preflight_deck());
    to_cabin(&mut session);
    click(&mut session, Action::OpenBriefing);

    let replay = click(&mut session, Action::ReplayBriefing);
    assert_eq!(session.front_end().screen(), Screen::Briefing);

    let recon = click(&mut session, Action::OpenRecon);
    assert_eq!(session.front_end().screen(), Screen::Recon);

    let back = click(&mut session, Action::Back);
    assert_eq!(session.front_end().screen(), Screen::Briefing);

    for effects in [&replay, &recon, &back] {
        assert_eq!(requests(effects), Vec::<Request>::new());
        assert!(!effects.contains(&Effect::Acquire(Resource::World)));
        assert!(
            !session.front_end().held().contains(&Resource::World),
            "the mission never started, so no world is held"
        );
    }

    click(&mut session, Action::Back);
    assert_eq!(session.front_end().screen(), Screen::Cabin);
    assert!(!session.front_end().held().contains(&Resource::World));
}

/// Cancelling a load (the one preflight screen that is entered by a
/// transaction) asks for nothing beyond the launch, releases the half-loaded
/// world, keeps the selection for the retry and leaves the campaign alone.
#[test]
fn accept_f45_b_cancelling_the_load_keeps_the_selection_and_releases_the_world() {
    let campaign = campaign();
    let before = campaign.snapshot();

    let mut session = ScreenSession::new(preflight_deck());
    session.set_wingmate_required(true);
    // Entering the profile is the first transaction, made here.
    assert_eq!(
        to_cabin(&mut session),
        vec![Request::OpenProfile(ProfileIntent::New)]
    );
    click(&mut session, Action::OpenBriefing);
    click(&mut session, Action::ContinueToFlightCheck);
    session.select_loadout(good_loadout()).expect("select");

    // Launching commits aircraft and ammunition as one transaction.
    let launch = click(&mut session, Action::Launch);
    assert_eq!(
        requests(&launch),
        vec![Request::CommitLoadout(good_loadout())]
    );
    assert_eq!(session.front_end().screen(), Screen::Loading);
    assert!(session.front_end().held().contains(&Resource::World));

    let cancel = click(&mut session, Action::Cancel);
    assert_eq!(
        requests(&cancel),
        Vec::<Request>::new(),
        "cancelling the load asks for nothing more"
    );
    assert!(cancel.contains(&Effect::Release(Resource::World)));
    assert_eq!(session.front_end().screen(), Screen::FlightCheck);
    assert_eq!(session.front_end().loadout(), &good_loadout());
    assert!(!session.front_end().held().contains(&Resource::World));
    assert_eq!(
        campaign.snapshot(),
        before,
        "cancelling the load touched the campaign"
    );
}

/// The authored fixture really is what the deck validates: a malformed
/// hotspot never reaches a deck at all.
#[test]
fn accept_f45_b_an_out_of_bounds_hotspot_never_reaches_a_deck() {
    let layout = ScreenLayout::new(
        IMAGE,
        vec![hotspot(
            "cabin.back",
            "back",
            rect(0, 0, IMAGE.0 + 1, IMAGE.1),
        )],
    );
    assert!(matches!(layout, Err(LayoutError::OutOfBounds { .. })));
    assert!(
        preflight_deck()
            .assets(Screen::Cabin)
            .is_some_and(|assets| !assets.buttons().is_empty())
    );
}

/// The fixture ids stay what the tests read them as: an image for artwork, a
/// ui-resource for buttons.
#[test]
fn accept_f45_b_artwork_is_an_image_id_and_buttons_are_ui_resource_ids() {
    let assets = preflight_deck()
        .assets(Screen::Cabin)
        .expect("the cabin is authored")
        .clone();
    assert_eq!(assets.art().kind(), ContentKind::Image);
    assert_eq!(assets.image(), IMAGE);
    for button in assets.buttons() {
        assert_eq!(button.id.kind(), ContentKind::UiResource);
    }
    let wrong: ContentId = id(ContentKind::UiResource, "cabin-art");
    assert!(matches!(
        ScreenAssets::new(
            wrong,
            &ScreenLayout::new(
                IMAGE,
                vec![hotspot("cabin.back", "back", rect(0, 0, 10, 10))]
            )
            .expect("layout")
        ),
        Err(ScreenAssetError::ArtNotImage { .. })
    ));
}
