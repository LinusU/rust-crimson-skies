use cs_app::accessibility::navigation::{
    NavDevice, NavError, NavStep, Navigator, navigation_gaps, navigation_map,
};
use cs_app::ui::front_end::{
    Action, ConstructionDraft, Effect, FrontEnd, Loadout, Request, Screen,
};
use cs_types::content::{ContentId, ContentKind};
use cs_types::input::{
    ActionMap, Binding, BindingSource, BindingTarget, GamepadButton, Key, UiAction,
};

fn id(kind: ContentKind, key: &str) -> ContentId {
    ContentId::from_source(kind, key).expect("id")
}

struct Keys {
    device: NavDevice,
    down: BindingSource,
    confirm: BindingSource,
    cancel: BindingSource,
    foreign: BindingSource,
}

fn keyboard() -> Keys {
    Keys {
        device: NavDevice::Keyboard,
        down: BindingSource::Key(Key::ArrowDown),
        confirm: BindingSource::Key(Key::Enter),
        cancel: BindingSource::Key(Key::Backspace),
        foreign: BindingSource::GamepadButton(GamepadButton::South),
    }
}

fn controller() -> Keys {
    Keys {
        device: NavDevice::Controller,
        down: BindingSource::GamepadButton(GamepadButton::DpadDown),
        confirm: BindingSource::GamepadButton(GamepadButton::South),
        cancel: BindingSource::GamepadButton(GamepadButton::East),
        foreign: BindingSource::Key(Key::Enter),
    }
}

fn step(nav: &Navigator, fe: &mut FrontEnd, source: BindingSource) -> NavStep {
    nav.press(fe, source)
        .unwrap_or_else(|why| panic!("{source} on {:?}: {why}", fe.screen()))
}

fn good_loadout() -> Loadout {
    Loadout {
        player: Some(id(ContentKind::Airframe, "player-plane")),
        wingmate: None,
        ammunition: Some(id(ContentKind::Ammo, "standard")),
    }
}

/// Install selection to a launched mission, using one device only.
fn boot_to_launch(keys: &Keys) {
    let nav = Navigator::new(keys.device, navigation_map()).expect("navigator");
    let mut fe = FrontEnd::new();
    // The installation check is the application's, not a button.
    fe.apply(Action::InstallVerified).expect("install");
    assert_eq!(fe.screen(), Screen::MainMenu);

    // Main menu: New profile is first.
    step(&nav, &mut fe, keys.confirm);
    assert_eq!(fe.screen(), Screen::ProfileSelect);
    step(&nav, &mut fe, keys.confirm);
    assert_eq!(fe.screen(), Screen::Cabin);
    step(&nav, &mut fe, keys.confirm);
    assert_eq!(fe.screen(), Screen::Briefing);

    // Briefing: replay, recon, continue. Down twice reaches Continue.
    assert_eq!(step(&nav, &mut fe, keys.down), NavStep::Focus);
    step(&nav, &mut fe, keys.down);
    assert_eq!(fe.focus(), Some(Action::ContinueToFlightCheck));
    step(&nav, &mut fe, keys.confirm);
    assert_eq!(fe.screen(), Screen::FlightCheck);

    // Another device's press changes nothing.
    assert_eq!(step(&nav, &mut fe, keys.foreign), NavStep::Ignored);
    assert_eq!(fe.screen(), Screen::FlightCheck);

    fe.select_loadout(good_loadout()).expect("loadout");
    assert_eq!(fe.focus(), Some(Action::Launch));
    let NavStep::Applied(outcome) = step(&nav, &mut fe, keys.confirm) else {
        panic!("launch applies an action");
    };
    assert_eq!(fe.screen(), Screen::Loading);
    assert!(
        outcome
            .effects
            .contains(&Effect::Request(Request::CommitLoadout(good_loadout())))
    );
}

#[test]
fn accept_f52_a_keyboard_only_boot_to_launch() {
    boot_to_launch(&keyboard());
}

#[test]
fn accept_f52_a_controller_only_boot_to_launch() {
    boot_to_launch(&controller());
}

#[test]
fn accept_f52_a_the_designed_default_map_alone_strands_the_controller() {
    let default = ActionMap::designed_default();
    let missing = navigation_gaps(&default, NavDevice::Controller);
    assert!(missing.contains(&UiAction::Confirm), "{missing:?}");
    assert!(matches!(
        Navigator::new(NavDevice::Controller, default.clone()),
        Err(NavError::Gaps { .. })
    ));
    // The keyboard is already complete there, and the navigation map completes both.
    assert!(navigation_gaps(&default, NavDevice::Keyboard).is_empty());
    assert!(navigation_gaps(&navigation_map(), NavDevice::Controller).is_empty());
}

#[test]
fn accept_f52_a_a_map_without_confirm_for_the_device_is_refused() {
    let without: Vec<Binding> = navigation_map()
        .bindings()
        .iter()
        .copied()
        .filter(|b| {
            !(b.source == BindingSource::Key(Key::Enter)
                && b.target == BindingTarget::Ui(UiAction::Confirm))
        })
        .collect();
    let map = ActionMap::try_new(without).expect("map");
    match Navigator::new(NavDevice::Keyboard, map) {
        Err(NavError::Gaps { missing, .. }) => assert_eq!(missing, vec![UiAction::Confirm]),
        other => panic!("expected a gap, got {other:?}"),
    }
}

#[test]
fn accept_f52_a_cancel_never_quits_and_backs_out_where_it_can() {
    let keys = keyboard();
    let nav = Navigator::new(keys.device, navigation_map()).expect("navigator");
    let mut fe = FrontEnd::new();
    fe.apply(Action::InstallVerified).expect("install");
    // The main menu has Quit but neither Cancel nor Back.
    assert_eq!(
        nav.press(&mut fe, keys.cancel),
        Err(NavError::NoEscape {
            screen: Screen::MainMenu
        })
    );
    assert_eq!(fe.screen(), Screen::MainMenu);
    step(&nav, &mut fe, keys.confirm);
    step(&nav, &mut fe, keys.cancel);
    assert_eq!(fe.screen(), Screen::MainMenu);
}

#[test]
fn accept_f52_a_a_discard_prompt_is_answered_from_the_same_device() {
    let keys = controller();
    let nav = Navigator::new(keys.device, navigation_map()).expect("navigator");
    let mut fe = FrontEnd::new();
    fe.apply(Action::InstallVerified).expect("install");
    fe.apply(Action::NewProfile).expect("profile");
    fe.apply(Action::ConfirmProfile).expect("cabin");
    fe.apply(Action::OpenConstruction).expect("construction");
    fe.open_construction(ConstructionDraft {
        blueprint: id(ContentKind::Blueprint, "custom"),
        saved: vec![],
        components: vec![],
    })
    .expect("open");
    fe.edit_construction(vec![id(ContentKind::Weapon, "part")])
        .expect("edit");

    step(&nav, &mut fe, keys.cancel);
    assert!(fe.pending_discard().is_some());
    assert_eq!(fe.screen(), Screen::Construction);
    // Cancel at the prompt keeps editing; confirm discards and leaves.
    assert_eq!(step(&nav, &mut fe, keys.cancel), NavStep::KeptEditing);
    assert!(fe.pending_discard().is_none());
    step(&nav, &mut fe, keys.cancel);
    step(&nav, &mut fe, keys.confirm);
    assert_eq!(fe.screen(), Screen::Cabin);
}
