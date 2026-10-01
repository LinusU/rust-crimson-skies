use cs_app::ui::front_end::{
    Action, AudioScope, ConstructionDraft, Effect, FrontEnd, InputContext, Loadout, LoadoutProblem,
    MissionOutcome, ProfileIntent, Refusal, Request, Resource, Screen,
};
use cs_types::content::ContentKind;

use super::id;

fn press(front_end: &mut FrontEnd, action: Action) -> Vec<Effect> {
    front_end
        .apply(action)
        .unwrap_or_else(|why| panic!("{action:?} on {:?}: {why}", front_end.screen()))
        .effects
}

fn requests(effects: &[Effect]) -> Vec<&Request> {
    effects
        .iter()
        .filter_map(|effect| match effect {
            Effect::Request(request) => Some(request),
            _ => None,
        })
        .collect()
}

fn good_loadout() -> Loadout {
    Loadout {
        player: Some(id(ContentKind::Airframe, "player-plane")),
        wingmate: Some(id(ContentKind::Airframe, "wing-plane")),
        ammunition: Some(id(ContentKind::Ammo, "standard")),
    }
}

/// Fresh install selection to the flight check, buttons only.
fn to_flight_check(front_end: &mut FrontEnd) {
    press(front_end, Action::InstallVerified);
    press(front_end, Action::NewProfile);
    press(front_end, Action::ConfirmProfile);
    press(front_end, Action::OpenBriefing);
    press(front_end, Action::ContinueToFlightCheck);
    assert_eq!(front_end.screen(), Screen::FlightCheck);
}

#[test]
fn accept_f45_a_fresh_profile_to_first_flight_and_back_by_buttons_only() {
    let mut front_end = FrontEnd::new();
    front_end.set_wingmate_required(true);
    assert_eq!(front_end.screen(), Screen::InstallSelect);

    press(&mut front_end, Action::InstallVerified);
    assert_eq!(front_end.screen(), Screen::MainMenu);
    press(&mut front_end, Action::NewProfile);
    let effects = press(&mut front_end, Action::ConfirmProfile);
    assert_eq!(
        requests(&effects),
        vec![&Request::OpenProfile(ProfileIntent::New)]
    );
    assert_eq!(front_end.screen(), Screen::Cabin);

    press(&mut front_end, Action::OpenBriefing);
    // Replaying the briefing and looking at recon never start the mission.
    press(&mut front_end, Action::ReplayBriefing);
    press(&mut front_end, Action::OpenRecon);
    assert_eq!(front_end.screen(), Screen::Recon);
    assert!(!front_end.held().contains(&Resource::World));
    press(&mut front_end, Action::Back);
    assert_eq!(front_end.screen(), Screen::Briefing);
    press(&mut front_end, Action::ContinueToFlightCheck);

    front_end.select_loadout(good_loadout()).expect("select");
    let effects = press(&mut front_end, Action::Launch);
    // Aircraft and ammunition are selected by one request.
    assert_eq!(
        requests(&effects),
        vec![&Request::CommitLoadout(good_loadout())]
    );
    assert_eq!(front_end.screen(), Screen::Loading);
    assert!(front_end.held().contains(&Resource::World));

    press(&mut front_end, Action::LoadSucceeded);
    assert_eq!(front_end.screen(), Screen::Flight);
    assert!(
        front_end
            .held()
            .contains(&Resource::Input(InputContext::Flight))
    );

    // Pause -> settings -> resume.
    press(&mut front_end, Action::Pause);
    press(&mut front_end, Action::OpenSettings);
    assert_eq!(front_end.screen(), Screen::PauseSettings);
    press(&mut front_end, Action::Back);
    press(&mut front_end, Action::Resume);
    assert_eq!(front_end.screen(), Screen::Flight);

    let effects = press(&mut front_end, Action::MissionSucceeded);
    assert_eq!(
        requests(&effects),
        vec![&Request::ApplyOutcome(MissionOutcome::Success)]
    );
    // Success -> scrapbook -> cabin -> menu: back out the way the player came.
    press(&mut front_end, Action::OpenScrapbook);
    press(&mut front_end, Action::Back);
    assert_eq!(front_end.screen(), Screen::Cabin);
    let effects = press(&mut front_end, Action::Back);
    assert_eq!(requests(&effects), vec![&Request::CloseProfile]);
    assert_eq!(front_end.screen(), Screen::MainMenu);
    let quit = press(&mut front_end, Action::Quit);
    assert_eq!(quit, vec![Effect::ExitApplication]);
}

#[test]
fn accept_f45_a_returning_to_the_menu_releases_the_world_and_never_binds_input_twice() {
    let mut front_end = FrontEnd::new();
    to_flight_check(&mut front_end);
    front_end.select_loadout(good_loadout()).expect("select");
    let mut trace = Vec::new();
    trace.extend(press(&mut front_end, Action::Launch));
    trace.extend(press(&mut front_end, Action::LoadSucceeded));
    trace.extend(press(&mut front_end, Action::Pause));
    trace.extend(press(&mut front_end, Action::AbortMission));
    assert_eq!(front_end.screen(), Screen::Cabin);
    trace.extend(press(&mut front_end, Action::Back));

    assert!(!front_end.held().contains(&Resource::World));
    assert_eq!(
        trace
            .iter()
            .filter(|e| **e == Effect::Acquire(Resource::World))
            .count(),
        trace
            .iter()
            .filter(|e| **e == Effect::Release(Resource::World))
            .count(),
        "every world acquire is released"
    );
    // Exactly one input context is held at every point of the trace.
    let mut input = vec![InputContext::Menu];
    for effect in &trace {
        match effect {
            Effect::Release(Resource::Input(context)) => input.retain(|c| c != context),
            Effect::Acquire(Resource::Input(context)) => {
                assert!(
                    input.is_empty(),
                    "{context:?} bound while {input:?} is held"
                );
                input.push(*context);
            }
            _ => {}
        }
    }
    assert_eq!(input, vec![InputContext::Menu]);
    assert!(
        front_end
            .held()
            .contains(&Resource::Audio(AudioScope::FrontEnd))
    );
}

#[test]
fn accept_f45_a_cancel_at_every_preflight_screen_requests_nothing_from_the_domain() {
    // Each walk goes into a preflight screen and out through Cancel/Back.
    let walks: [(&[Action], Screen, Action); 5] = [
        (&[Action::OpenBriefing], Screen::Briefing, Action::Back),
        (
            &[Action::OpenBriefing, Action::OpenRecon],
            Screen::Recon,
            Action::Back,
        ),
        (&[Action::OpenScrapbook], Screen::Scrapbook, Action::Back),
        (
            &[Action::OpenConstruction],
            Screen::Construction,
            Action::Cancel,
        ),
        (
            &[Action::OpenBriefing, Action::ContinueToFlightCheck],
            Screen::FlightCheck,
            Action::Cancel,
        ),
    ];
    for (path, screen, leave) in walks {
        let mut front_end = FrontEnd::new();
        press(&mut front_end, Action::InstallVerified);
        press(&mut front_end, Action::ContinueProfile);
        press(&mut front_end, Action::ConfirmProfile);
        let mut effects = Vec::new();
        for step in path {
            effects.extend(press(&mut front_end, *step));
        }
        assert_eq!(front_end.screen(), screen);
        effects.extend(press(&mut front_end, leave));
        assert_eq!(requests(&effects), Vec::<&Request>::new(), "{screen:?}");
        assert_eq!(front_end.outcome(), None);
        assert!(front_end.loadout().is_empty());
    }
}

#[test]
fn accept_f45_a_back_from_a_dirty_draft_asks_and_keep_editing_loses_nothing() {
    let part = id(ContentKind::HardpointEquipment, "gun-mount");
    let mut front_end = FrontEnd::new();
    press(&mut front_end, Action::InstallVerified);
    press(&mut front_end, Action::NewProfile);
    press(&mut front_end, Action::ConfirmProfile);
    press(&mut front_end, Action::OpenConstruction);
    front_end
        .open_construction(ConstructionDraft {
            blueprint: id(ContentKind::Blueprint, "custom"),
            saved: vec![],
            components: vec![],
        })
        .expect("open");

    // A clean draft leaves at once.
    let mut clean = front_end.clone();
    press(&mut clean, Action::Cancel);
    assert_eq!(clean.screen(), Screen::Cabin);

    front_end
        .edit_construction(vec![part.clone()])
        .expect("edit");
    let effects = press(&mut front_end, Action::Cancel);
    assert_eq!(effects, vec![Effect::AskDiscard]);
    assert_eq!(front_end.screen(), Screen::Construction);
    // Nothing else moves while the prompt is open.
    assert_eq!(
        front_end.apply(Action::CommitConstruction),
        Err(Refusal::ConfirmationPending)
    );
    front_end.keep_editing().expect("keep");
    assert_eq!(
        front_end.construction().expect("draft").components,
        vec![part]
    );
    assert_eq!(front_end.keep_editing(), Err(Refusal::NoPendingDiscard));

    press(&mut front_end, Action::Back);
    let left = front_end.confirm_discard().expect("discard");
    assert_eq!(left.to, Screen::Cabin);
    assert_eq!(requests(&left.effects), Vec::<&Request>::new());
    assert_eq!(front_end.construction(), None);
}

#[test]
fn accept_f45_a_commit_construction_requests_the_blueprint_and_does_not_prompt() {
    let part = id(ContentKind::HardpointEquipment, "gun-mount");
    let mut front_end = FrontEnd::new();
    press(&mut front_end, Action::InstallVerified);
    press(&mut front_end, Action::NewProfile);
    press(&mut front_end, Action::ConfirmProfile);
    press(&mut front_end, Action::OpenConstruction);
    // Committing without a draft is refused and leaves the screen.
    assert_eq!(
        front_end.apply(Action::CommitConstruction),
        Err(Refusal::NoDraft)
    );
    assert_eq!(front_end.screen(), Screen::Construction);
    let draft = ConstructionDraft {
        blueprint: id(ContentKind::Blueprint, "custom"),
        saved: vec![],
        components: vec![part],
    };
    front_end.open_construction(draft.clone()).expect("open");
    let effects = press(&mut front_end, Action::CommitConstruction);
    assert_eq!(requests(&effects), vec![&Request::CommitBlueprint(draft)]);
    assert_eq!(front_end.screen(), Screen::Cabin);
}

#[test]
fn accept_f45_a_invalid_loadout_is_refused_until_corrected_then_launches() {
    let mut front_end = FrontEnd::new();
    front_end.set_wingmate_required(true);
    to_flight_check(&mut front_end);

    let mut bad = good_loadout();
    bad.wingmate = None;
    bad.ammunition = Some(id(ContentKind::Gun, "not-ammo"));
    front_end.select_loadout(bad).expect("select");
    let refusal = front_end.apply(Action::Launch).expect_err("refused");
    let Refusal::InvalidLoadout(problems) = refusal else {
        panic!("wrong refusal");
    };
    assert!(problems.contains(&LoadoutProblem::MissingWingmate));
    assert!(problems.iter().any(|p| matches!(
        p,
        LoadoutProblem::WrongKind {
            slot: "ammunition",
            ..
        }
    )));
    assert_eq!(
        front_end.screen(),
        Screen::FlightCheck,
        "a refusal changes nothing"
    );

    front_end.select_loadout(good_loadout()).expect("select");
    press(&mut front_end, Action::Launch);
    assert_eq!(front_end.screen(), Screen::Loading);

    // Without the declaration the wingmate is not assumed required.
    let mut lone = FrontEnd::new();
    to_flight_check(&mut lone);
    let mut solo = good_loadout();
    solo.wingmate = None;
    lone.select_loadout(solo).expect("select");
    press(&mut lone, Action::Launch);
    assert_eq!(lone.screen(), Screen::Loading);
}

#[test]
fn accept_f45_a_a_failed_load_keeps_the_selection_and_retries_without_a_restart() {
    let mut front_end = FrontEnd::new();
    front_end.set_wingmate_required(true);
    to_flight_check(&mut front_end);
    front_end.select_loadout(good_loadout()).expect("select");
    press(&mut front_end, Action::Launch);

    let failed = front_end
        .report_load_failure("dependency missing")
        .expect("failure");
    assert_eq!(failed.to, Screen::FlightCheck);
    assert!(
        failed.effects.contains(&Effect::Release(Resource::World)),
        "the half-loaded world is released"
    );
    assert_eq!(front_end.loadout(), &good_loadout(), "draft survives");
    assert_eq!(
        front_end.last_failure().map(|f| f.reason.as_str()),
        Some("dependency missing")
    );

    // After the dependency is repaired the same selection launches again.
    let retry = press(&mut front_end, Action::Launch);
    assert_eq!(
        requests(&retry),
        vec![&Request::CommitLoadout(good_loadout())]
    );
    assert_eq!(front_end.last_failure(), None, "cleared by the new load");
    press(&mut front_end, Action::LoadSucceeded);
    assert_eq!(front_end.screen(), Screen::Flight);
}

#[test]
fn accept_f45_a_cancel_while_loading_returns_to_the_flight_check_with_the_draft() {
    let mut front_end = FrontEnd::new();
    to_flight_check(&mut front_end);
    front_end.select_loadout(good_loadout()).expect("select");
    press(&mut front_end, Action::Launch);
    let effects = press(&mut front_end, Action::Cancel);
    assert!(effects.contains(&Effect::Release(Resource::World)));
    assert_eq!(front_end.screen(), Screen::FlightCheck);
    assert_eq!(front_end.loadout(), &good_loadout());
}

#[test]
fn accept_f45_a_retry_is_offered_after_failure_only() {
    let mut front_end = FrontEnd::new();
    to_flight_check(&mut front_end);
    front_end.select_loadout(good_loadout()).expect("select");
    press(&mut front_end, Action::Launch);
    press(&mut front_end, Action::LoadSucceeded);
    press(&mut front_end, Action::MissionFailed);
    assert_eq!(front_end.outcome(), Some(MissionOutcome::Failure));
    press(&mut front_end, Action::Retry);
    assert_eq!(front_end.screen(), Screen::Loading);
    assert_eq!(front_end.loadout(), &good_loadout(), "same loadout retried");

    press(&mut front_end, Action::LoadSucceeded);
    press(&mut front_end, Action::MissionSucceeded);
    assert_eq!(front_end.apply(Action::Retry), Err(Refusal::NotAFailure));
    assert_eq!(front_end.screen(), Screen::Results);
    press(&mut front_end, Action::ReturnToMenu);
    assert_eq!(front_end.screen(), Screen::MainMenu);
    assert_eq!(front_end.outcome(), None);
}

#[test]
fn accept_f45_a_missing_content_goes_through_diagnosis_back_to_selection() {
    let mut front_end = FrontEnd::new();
    press(&mut front_end, Action::InstallRejected);
    assert_eq!(front_end.screen(), Screen::ContentDiagnosis);
    press(&mut front_end, Action::ChooseAnotherInstall);
    assert_eq!(front_end.screen(), Screen::InstallSelect);
    press(&mut front_end, Action::InstallVerified);
    press(&mut front_end, Action::ContentMissing);
    assert_eq!(front_end.screen(), Screen::ContentDiagnosis);
}

#[test]
fn accept_f45_a_an_action_the_screen_does_not_offer_is_refused_and_changes_nothing() {
    let mut front_end = FrontEnd::new();
    let before = front_end.screen();
    assert_eq!(
        front_end.apply(Action::Launch),
        Err(Refusal::NoTransition {
            screen: before,
            action: Action::Launch
        })
    );
    // The mission cannot start from the cabin or the menu.
    press(&mut front_end, Action::InstallVerified);
    assert!(front_end.apply(Action::LoadSucceeded).is_err());
    assert_eq!(front_end.screen(), Screen::MainMenu);
    assert!(front_end.select_loadout(good_loadout()).is_err());
}

#[test]
fn accept_f45_a_focus_resets_on_entry_wraps_and_activates_a_working_button() {
    let mut front_end = FrontEnd::new();
    press(&mut front_end, Action::InstallVerified);
    assert_eq!(front_end.screen(), Screen::MainMenu);
    let visible = front_end.visible_actions();
    assert_eq!(
        visible,
        vec![
            Action::NewProfile,
            Action::ContinueProfile,
            Action::OpenSettings,
            Action::Quit
        ]
    );
    assert_eq!(front_end.focus(), Some(Action::NewProfile));
    front_end.move_focus(false);
    assert_eq!(front_end.focus(), Some(Action::Quit), "wraps backward");
    front_end.move_focus(true);
    assert_eq!(front_end.focus(), Some(Action::NewProfile), "wraps forward");
    front_end.move_focus(true);
    front_end.move_focus(true);
    assert_eq!(front_end.focus(), Some(Action::OpenSettings));

    front_end.activate_focus().expect("activate");
    assert_eq!(front_end.screen(), Screen::Settings);
    assert_eq!(front_end.focus(), Some(Action::Back), "reset on entry");

    // Every visible button of every reachable screen works from that screen.
    for action in front_end.visible_actions() {
        let mut probe = front_end.clone();
        assert!(probe.apply(action).is_ok(), "{action:?}");
    }
}

#[test]
fn accept_f45_a_every_visible_button_has_a_transition_on_its_screen() {
    use cs_app::ui::front_end::{ActionSource, rows_on};
    for screen in Screen::ALL {
        let buttons: Vec<_> = rows_on(screen)
            .filter(|row| row.action.source() == ActionSource::User)
            .collect();
        assert!(!buttons.is_empty(), "{screen:?} shows no button");
    }
}
