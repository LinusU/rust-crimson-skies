//! Acceptance stage F49-C2 (#776): the Instant Action screens wired into the
//! front-end state table and driven by `FrontEndFlow`. Task test prefix:
//! `accept_f49_c_2_`. Shared contracts: `docs/contracts/UI-NETWORK.md` ("Use an
//! explicit state table for every screen and Back/Cancel path", the path "IA
//! customize -> finish -> main") and `docs/contracts/STATE-TRANSACTIONS.md`
//! (`end_session` for a generation, generation-qualified results).
//!
//! The minimum scenario is
//! `accept_f49_c_2_main_menu_to_instant_action_and_back_to_the_menu`: from the
//! main menu, by clicking authored buttons, select a preset, launch, finish a
//! session as a defeat, retry, win, finish — and the resource ledger must have
//! held exactly one session generation at a time, released each exactly once,
//! and hold none at the menu. The rest cover the other branches of the path, a
//! domain refusal leaving screen, focus and ledger where they were, the
//! ledger's own rules, and the table's totality with the new rows.
//!
//! All data is authored synthetic fixture data (the F49-A catalog fixture and
//! F45's fixture decks); nothing here says how the original's Instant Action
//! screens were laid out.

use cs_app::campaign::lower_campaign;
use cs_app::ui::front_end::{
    Action, ActionSource, AudioScope, FlowError, FlowSetup, FrontEndFlow, Guard, InputContext,
    InstantActionInputs, Refusal, Request, RequestKind, Resource, ResourceLedger, ResourceProblem,
    Screen, TABLE, find, review_navigation, validate_table,
};
use cs_app::ui::instant_action::{
    CustomDimension, IA_TABLE, IaActionKind, IaEdit, IaEffect, IaRefusal, IaScreen, ScenarioResult,
    ScenarioSnapshot,
};
use cs_content::ai::DifficultyTier;
use cs_content::campaign::declared_synthetic_campaign;
use cs_content::instant_action::{
    RosterSlot, ScenarioSide, synthetic_dogfight_preset, synthetic_instant_action_catalog,
};
use cs_sim::campaign::{CampaignRunId, DifficultyId, ProfileId, SessionGeneration};
use cs_types::content::{ContentId, ContentKind};

use super::screens::{SURFACE, button_point, instant_action_deck};

fn profile() -> ProfileId {
    ProfileId::new("pilot.nathan").expect("a valid profile id")
}

fn custom_subject() -> ContentId {
    ContentId::from_source(
        ContentKind::IaScenario,
        "synthetic.fixture_ia_scenario_custom",
    )
    .expect("a valid scenario id")
}

fn setup() -> FlowSetup {
    FlowSetup::new(
        std::env::temp_dir().join("cs-f49-c-2-no-profile-is-opened"),
        lower_campaign(&declared_synthetic_campaign()).expect("the fixture campaign lowers"),
        CampaignRunId::new("run.f49c2").expect("a valid run id"),
        DifficultyId::new("standard").expect("a valid difficulty"),
    )
    .sandbox()
}

fn inputs() -> InstantActionInputs {
    InstantActionInputs {
        profile: profile(),
        catalog: synthetic_instant_action_catalog(),
        custom_subject: custom_subject(),
    }
}

/// A flow on the main menu with the Instant Action inputs supplied.
fn at_menu() -> FrontEndFlow {
    let mut flow = FrontEndFlow::new(instant_action_deck(), setup().instant_action(inputs()));
    flow.press(Action::InstallVerified)
        .expect("the install verifies");
    assert_eq!(flow.front_end().screen(), Screen::MainMenu);
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

fn dogfight() -> ContentId {
    synthetic_dogfight_preset().id().clone()
}

fn subject_of(flow: &FrontEndFlow, preset: &ContentId) -> ContentId {
    flow.domain()
        .instant_action()
        .expect("the flow was given a catalog")
        .preset_rows()
        .iter()
        .find(|row| row.preset_id == *preset)
        .expect("the catalog lists the preset")
        .scenario_id
        .clone()
}

fn live_generation(flow: &FrontEndFlow) -> SessionGeneration {
    flow.domain()
        .instant_action()
        .and_then(|ia| ia.generation())
        .expect("a session is running")
}

/// The player's side is destroyed.
fn defeat() -> ScenarioSnapshot {
    ScenarioSnapshot::new(9, vec![(ScenarioSide::Enemy, RosterSlot(1))])
}

/// Every enemy is destroyed.
fn victory() -> ScenarioSnapshot {
    ScenarioSnapshot::new(20, vec![(ScenarioSide::Player, RosterSlot(0))])
}

/// What a refusal must not have moved.
#[derive(Debug, PartialEq)]
struct Snapshot {
    screen: Screen,
    focus: Option<Action>,
    held: std::collections::BTreeSet<Resource>,
    ledger: ResourceLedger,
    ia_screen: Option<IaScreen>,
    generation: Option<SessionGeneration>,
    records: usize,
    pending: Option<Action>,
}

fn snapshot(flow: &FrontEndFlow) -> Snapshot {
    let domain = flow.domain();
    let ia = domain.instant_action();
    Snapshot {
        screen: flow.front_end().screen(),
        focus: flow.front_end().focus(),
        held: flow.front_end().held().clone(),
        ledger: flow.ledger().clone(),
        ia_screen: ia.map(|ia| ia.screen()),
        generation: ia.and_then(|ia| ia.generation()),
        records: ia.map_or(0, |ia| ia.records().len()),
        pending: flow.front_end().pending_discard(),
    }
}

#[test]
fn accept_f49_c_2_main_menu_to_instant_action_and_back_to_the_menu() {
    let mut flow = at_menu();
    assert_eq!(flow.ledger().session(), None);

    click(&mut flow, Action::OpenInstantAction);
    assert_eq!(flow.front_end().screen(), Screen::IaSelect);
    assert_eq!(
        flow.ledger().session(),
        None,
        "selecting holds no session yet"
    );

    let preset = dogfight();
    let subject = subject_of(&flow, &preset);
    flow.select_instant_action_preset(&preset)
        .expect("the preset selects");

    // Launch: one generation, the world and the flight input context.
    click(&mut flow, Action::Launch);
    assert_eq!(flow.front_end().screen(), Screen::IaFlight);
    let first = live_generation(&flow);
    assert_eq!(flow.ledger().session(), Some(first));
    assert!(flow.ledger().holds_world());
    assert_eq!(flow.ledger().input(), Some(InputContext::Flight));
    assert_eq!(flow.ledger().audio(), Some(AudioScope::Flight));

    // The session ends as a defeat: results, the world is released but the
    // generation stays live until it is retried or finished.
    flow.report_instant_action(first, defeat())
        .expect("the session settles");
    assert_eq!(flow.front_end().screen(), Screen::IaResults);
    assert_eq!(flow.ledger().session(), Some(first));
    assert!(!flow.ledger().holds_world());
    assert_eq!(
        flow.domain()
            .instant_action()
            .and_then(|ia| ia.outcome())
            .map(|outcome| outcome.result()),
        Some(ScenarioResult::Defeat)
    );

    // Retry: the old generation is released before the new one is acquired.
    click(&mut flow, Action::Retry);
    assert_eq!(flow.front_end().screen(), Screen::IaFlight);
    let second = live_generation(&flow);
    assert_ne!(first, second, "a generation is never reused");
    assert_eq!(flow.ledger().session(), Some(second));
    assert_eq!(flow.ledger().sessions_begun(), 2);
    assert!(flow.ledger().holds_world());

    flow.report_instant_action(second, victory())
        .expect("the retry settles");
    click(&mut flow, Action::Finish);

    // Back at the menu: nothing of the session is held, once.
    assert_eq!(flow.front_end().screen(), Screen::MainMenu);
    assert_eq!(flow.ledger().session(), None);
    assert!(!flow.ledger().holds_world());
    assert_eq!(flow.ledger().input(), Some(InputContext::Menu));
    assert_eq!(flow.ledger().audio(), Some(AudioScope::FrontEnd));
    assert_eq!(flow.ledger().resources(), *flow.front_end().held());
    let domain = flow.domain();
    let ia = domain.instant_action().expect("the flow lives on");
    assert_eq!(ia.screen(), IaScreen::Select);
    assert_eq!(ia.records().len(), 1, "the finished session was recorded");
    assert_eq!(ia.records().entries()[0].subject(), &subject);
    assert_eq!(ia.records().entries()[0].result(), ScenarioResult::Victory);

    // A second visit: a fresh generation, never one a stored entry holds.
    click(&mut flow, Action::OpenInstantAction);
    flow.select_instant_action_preset(&preset)
        .expect("the preset selects");
    click(&mut flow, Action::Launch);
    let third = live_generation(&flow);
    assert!(third > second);
    assert_eq!(flow.ledger().sessions_begun(), 3);
}

#[test]
fn accept_f49_c_2_abandoning_a_flight_releases_its_generation_exactly_once() {
    let mut flow = at_menu();
    flow.press(Action::OpenInstantAction).expect("opens");
    flow.select_instant_action_preset(&dogfight())
        .expect("selects");
    flow.press(Action::Launch).expect("launches");
    let generation = live_generation(&flow);

    let outcome = flow.press(Action::Back).expect("the flight is abandoned");
    assert_eq!(outcome.to, Screen::IaSelect);
    assert_eq!(flow.ledger().session(), None);
    assert!(!flow.ledger().holds_world());
    assert_eq!(flow.ledger().sessions_begun(), 1);
    assert_eq!(
        flow.domain()
            .instant_action()
            .map(|ia| ia.records().len())
            .unwrap_or(usize::MAX),
        0,
        "an abandoned flight records nothing"
    );

    // Leaving for the menu releases nothing a second time.
    flow.press(Action::Back).expect("back to the menu");
    assert_eq!(flow.front_end().screen(), Screen::MainMenu);
    assert_eq!(flow.ledger().session(), None);

    // The ledger would refuse the second release outright.
    let mut ledger = flow.ledger().clone();
    assert_eq!(
        ledger.apply_ia(&IaEffect::EndSession { generation }),
        Err(ResourceProblem::SessionEndedUnknown(generation))
    );
}

#[test]
fn accept_f49_c_2_customize_launch_abandon_and_leave_asks_before_discarding() {
    let mut flow = at_menu();
    flow.press(Action::OpenInstantAction).expect("opens");
    flow.select_instant_action_preset(&dogfight())
        .expect("selects");
    flow.press(Action::StartCustom).expect("the draft opens");
    assert_eq!(flow.front_end().screen(), Screen::IaCustomize);

    // Edit-free Back would leave at once; an edit makes it ask.
    let domain = flow.domain();
    let ia = domain.instant_action().expect("the flow lives on");
    let tier = ia
        .draft()
        .and_then(|draft| draft.difficulty())
        .expect("the seeded tier")
        .tier();
    let other = ia
        .dimensions()
        .iter()
        .find_map(|dimension| match dimension {
            CustomDimension::Difficulty(values) => {
                values.iter().copied().find(|value| *value != tier)
            }
            _ => None,
        })
        .expect("the fixture offers a second tier");
    flow.edit_instant_action(IaEdit::Difficulty(other))
        .expect("the edit is accepted");
    assert!(flow.front_end().instant_action_draft_is_dirty());

    // Launch the edited draft, abandon the flight: the draft comes back to the
    // selection screen still dirty, and leaving for the menu asks.
    flow.press(Action::Launch).expect("the draft launches");
    assert_eq!(flow.front_end().screen(), Screen::IaFlight);
    flow.press(Action::Back).expect("the flight is abandoned");
    assert_eq!(flow.front_end().screen(), Screen::IaSelect);
    assert!(flow.front_end().instant_action_draft_is_dirty());

    let before = snapshot(&flow);
    let asked = flow.press(Action::Back).expect("the prompt opens");
    assert_eq!(asked.to, Screen::IaSelect);
    assert_eq!(flow.front_end().pending_discard(), Some(Action::Back));
    // Keep editing: nothing moved but the prompt.
    flow.keep_editing().expect("keeps editing");
    assert_eq!(snapshot(&flow), before);

    flow.press(Action::Back).expect("the prompt opens again");
    let left = flow.confirm_discard().expect("the discard is confirmed");
    assert_eq!(left.to, Screen::MainMenu);
    let domain = flow.domain();
    let ia = domain.instant_action().expect("the flow lives on");
    assert_eq!(ia.screen(), IaScreen::Select);
    assert!(!ia.draft_is_dirty());
    assert!(ia.draft().is_none(), "the draft was dropped");
    assert!(!flow.front_end().instant_action_draft_is_dirty());
    assert_eq!(flow.ledger().session(), None);
}

#[test]
fn accept_f49_c_2_a_clean_customize_back_asks_nothing() {
    let mut flow = at_menu();
    flow.press(Action::OpenInstantAction).expect("opens");
    flow.select_instant_action_preset(&dogfight())
        .expect("selects");
    flow.press(Action::StartCustom).expect("the draft opens");
    let outcome = flow.press(Action::Back).expect("leaves at once");
    assert_eq!(outcome.to, Screen::IaSelect);
    assert_eq!(flow.front_end().pending_discard(), None);
    assert_eq!(
        flow.domain().instant_action().map(|ia| ia.screen()),
        Some(IaScreen::Select)
    );
}

#[test]
fn accept_f49_c_2_a_domain_refusal_leaves_screen_focus_and_ledger_untouched() {
    let mut flow = at_menu();
    flow.press(Action::OpenInstantAction).expect("opens");

    // Launch and custom with nothing selected.
    let before = snapshot(&flow);
    let launch = flow.press(Action::Launch).expect_err("nothing is selected");
    assert!(
        matches!(launch, FlowError::InstantAction(IaRefusal::NoSelection)),
        "{launch:?}"
    );
    let custom = flow
        .press(Action::StartCustom)
        .expect_err("no preset to start from");
    assert!(
        matches!(
            custom,
            FlowError::InstantAction(IaRefusal::NoPresetSelected)
        ),
        "{custom:?}"
    );
    assert_eq!(snapshot(&flow), before);

    // A selection the catalog cannot lower.
    let unknown =
        ContentId::from_source(ContentKind::IaPreset, "synthetic.fixture_ia_missing_preset")
            .expect("a valid preset id");
    flow.select_instant_action_preset(&unknown)
        .expect("the selection is not checked early");
    let before = snapshot(&flow);
    let refused = flow.press(Action::Launch).expect_err("not lowerable");
    assert!(
        matches!(refused, FlowError::InstantAction(IaRefusal::Lower(_))),
        "{refused:?}"
    );
    assert_eq!(snapshot(&flow), before);
    assert_eq!(flow.ledger().sessions_begun(), 0);

    // A flight: a report from another generation or for a scenario that has
    // not ended, and a bare system press with no report at all.
    flow.select_instant_action_preset(&dogfight())
        .expect("selects");
    flow.press(Action::Launch).expect("launches");
    let generation = live_generation(&flow);
    let unfinished = flow
        .domain()
        .instant_action()
        .and_then(|ia| ia.authored_snapshot())
        .expect("a session is running");
    let before = snapshot(&flow);

    let stale = flow
        .report_instant_action(SessionGeneration(generation.0 + 7), defeat())
        .expect_err("another generation's callback");
    assert!(
        matches!(
            stale,
            FlowError::InstantAction(IaRefusal::StaleGeneration { .. })
        ),
        "{stale:?}"
    );
    let running = flow
        .report_instant_action(generation, unfinished)
        .expect_err("the scenario has not ended");
    assert!(
        matches!(running, FlowError::InstantAction(_)),
        "{running:?}"
    );
    let bare = flow
        .press(Action::InstantActionEnded)
        .expect_err("no report is waiting");
    assert!(matches!(bare, FlowError::NoInstantActionReport), "{bare:?}");
    assert_eq!(snapshot(&flow), before);
    assert_eq!(flow.ledger().session(), Some(generation));
}

#[test]
fn accept_f49_c_2_without_a_catalog_the_menu_refuses_to_enter() {
    let mut flow = FrontEndFlow::new(instant_action_deck(), setup());
    flow.press(Action::InstallVerified).expect("verifies");
    let before = snapshot(&flow);
    let refused = flow
        .press(Action::OpenInstantAction)
        .expect_err("nothing to select from");
    assert!(matches!(refused, FlowError::NoInstantAction), "{refused:?}");
    assert_eq!(snapshot(&flow), before);
    assert_eq!(flow.front_end().screen(), Screen::MainMenu);
}

#[test]
fn accept_f49_c_2_the_wiring_calls_belong_to_their_screens() {
    let mut flow = at_menu();
    let wrong = flow
        .select_instant_action_preset(&dogfight())
        .expect_err("the menu is not the selection screen");
    assert!(
        matches!(
            wrong,
            FlowError::Screen(cs_app::ui::front_end::ScreenSessionError::Refused(
                Refusal::WrongScreen { .. }
            ))
        ),
        "{wrong:?}"
    );
    flow.press(Action::OpenInstantAction).expect("opens");
    let wrong = flow
        .report_instant_action(SessionGeneration(1), defeat())
        .expect_err("no flight is running");
    assert!(matches!(wrong, FlowError::Screen(_)), "{wrong:?}");
    let wrong = flow
        .edit_instant_action(IaEdit::Difficulty(DifficultyTier::Relaxed))
        .expect_err("the selection screen is not the customize screen");
    assert!(matches!(wrong, FlowError::Screen(_)), "{wrong:?}");
}

#[test]
fn accept_f49_c_2_the_ledger_never_holds_a_generation_twice() {
    let begin = |generation: u32| IaEffect::BeginSession {
        generation: SessionGeneration(generation),
        subject: custom_subject(),
    };
    let end = |generation: u32| IaEffect::EndSession {
        generation: SessionGeneration(generation),
    };
    let mut ledger = ResourceLedger::default();
    ledger.apply_ia(&begin(1)).expect("the first begins");
    assert_eq!(
        ledger.apply_ia(&begin(2)),
        Err(ResourceProblem::SessionHeldTwice {
            held: SessionGeneration(1),
            offered: SessionGeneration(2)
        }),
        "a second generation cannot begin before the first ends"
    );
    assert_eq!(ledger.session(), Some(SessionGeneration(1)));
    assert_eq!(
        ledger.apply_ia(&end(2)),
        Err(ResourceProblem::SessionEndedUnknown(SessionGeneration(2)))
    );
    ledger.apply_ia(&end(1)).expect("the live one ends");
    assert_eq!(
        ledger.apply_ia(&end(1)),
        Err(ResourceProblem::SessionEndedUnknown(SessionGeneration(1))),
        "a generation is released exactly once"
    );
    assert_eq!(
        ledger.apply_ia(&begin(1)),
        Err(ResourceProblem::SessionGenerationReused(SessionGeneration(
            1
        ))),
        "an ended generation is not acquired again"
    );
    assert_eq!(ledger.session(), None);
    for harmless in [
        IaEffect::AskDiscard,
        IaEffect::DiscardDraft,
        IaEffect::LeaveToMenu,
    ] {
        ledger.apply_ia(&harmless).expect("holds nothing");
    }
    assert_eq!(ledger.sessions_begun(), 1);
}

/// The front-end screen an Instant Action screen is.
fn mirrored(screen: IaScreen) -> Screen {
    match screen {
        IaScreen::Select => Screen::IaSelect,
        IaScreen::Customize => Screen::IaCustomize,
        IaScreen::Flight => Screen::IaFlight,
        IaScreen::Results => Screen::IaResults,
    }
}

#[test]
fn accept_f49_c_2_the_table_mirrors_ia_table_and_stays_total() {
    assert_eq!(validate_table(), Vec::new());

    // Main menu -> Instant Action -> Select, and finish -> main.
    let entry = find(Screen::MainMenu, Action::OpenInstantAction).expect("an entry row");
    assert_eq!(entry.to, Screen::IaSelect);
    assert_eq!(entry.request, RequestKind::OpenInstantAction);
    assert_eq!(
        find(Screen::IaResults, Action::Finish).map(|row| row.to),
        Some(Screen::MainMenu)
    );

    // Every Instant Action row has its front-end row, to the same screen
    // (leaving the screens ends on the menu instead of the resting `Select`).
    for row in IA_TABLE {
        let action = match row.action {
            IaActionKind::StartCustom => Action::StartCustom,
            IaActionKind::Launch => Action::Launch,
            IaActionKind::Back => Action::Back,
            IaActionKind::Retry => Action::Retry,
            IaActionKind::Finish => Action::Finish,
            IaActionKind::Report => Action::InstantActionEnded,
            // The prompt's answers are the machine's own discard prompt.
            IaActionKind::ConfirmDiscard | IaActionKind::KeepEditing => continue,
        };
        let ours = find(mirrored(row.from), action)
            .unwrap_or_else(|| panic!("no front-end row for {row:?}"));
        let leaves = matches!(
            (row.from, row.action),
            (IaScreen::Select, IaActionKind::Back) | (IaScreen::Results, IaActionKind::Finish)
        );
        let expected = if leaves {
            Screen::MainMenu
        } else {
            mirrored(row.to)
        };
        assert_eq!(ours.to, expected, "{row:?}");
    }

    // The prompt rows are user-raised and ask the domain for the step they
    // would have taken.
    for row in TABLE {
        if row.guard == Guard::DiscardsScenarioDraft {
            assert_eq!(row.action.source(), ActionSource::User, "{row:?}");
            assert_eq!(row.request, RequestKind::InstantActionBack, "{row:?}");
        }
    }
    assert_eq!(
        Action::InstantActionEnded.source(),
        ActionSource::System,
        "a session ending is the application's result, not a button"
    );
}

#[test]
fn accept_f49_c_2_the_navigation_review_reaches_every_instant_action_screen() {
    let review = review_navigation(Screen::START);
    for screen in [
        Screen::IaSelect,
        Screen::IaCustomize,
        Screen::IaFlight,
        Screen::IaResults,
    ] {
        assert!(review.reached.contains(&screen), "{screen:?} not reached");
        assert!(!review.not_reached.contains(&screen));
    }
    assert!(review.is_complete(), "{review}");
    let requests = review.requests();
    assert!(requests.contains(&(
        Screen::MainMenu,
        Action::OpenInstantAction,
        RequestKind::OpenInstantAction
    )));
    assert!(requests.contains(&(
        Screen::IaResults,
        Action::Finish,
        RequestKind::FinishInstantAction
    )));
}

#[test]
fn accept_f49_c_2_every_instant_action_request_is_a_distinct_machine_request() {
    // The machine alone (no domain) names the step each press asks for.
    let mut machine = cs_app::ui::front_end::FrontEnd::new();
    machine.apply(Action::InstallVerified).expect("verifies");
    let plan = machine.plan(Action::OpenInstantAction).expect("plans");
    assert_eq!(plan.request, Some(Request::OpenInstantAction));
    assert_eq!(plan.to, Screen::IaSelect);
    machine.apply(Action::OpenInstantAction).expect("opens");
    assert_eq!(
        machine.plan(Action::StartCustom).expect("plans").request,
        Some(Request::StartCustomScenario)
    );
    assert_eq!(
        machine.plan(Action::Launch).expect("plans").request,
        Some(Request::LaunchInstantAction)
    );
    assert_eq!(
        machine.plan(Action::Back).expect("plans").request,
        Some(Request::InstantActionBack)
    );
    let held = |machine: &cs_app::ui::front_end::FrontEnd| machine.held().clone();
    machine.apply(Action::Launch).expect("launches");
    assert!(held(&machine).contains(&Resource::World));
    machine.apply(Action::Back).expect("abandons");
    assert!(!held(&machine).contains(&Resource::World));
}
