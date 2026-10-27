//! Acceptance stage F52-W3: the labelled modern control profile reaches the
//! flight input path, and a modified run names its assists in the replay
//! record (`specs/F52-accessibility-and-explicitly-separated-modern-options.md`,
//! AC03/AC04).
//!
//! Everything runs through production code: `Settings::gameplay_inputs()` is
//! the only input, `cs_app::input::assist` builds the action map, a real
//! `InputCollector` / `InputSession` resolves device events with it, and
//! `ReplaySubject::with_fidelity` feeds the real recorder. The modern schemes
//! are project-designed; no original binding or field of view is claimed.

use cs_app::capture::{BuildContext, ReplaySubject, RunRequest, record};
use cs_app::input::assist::{apply_assists, fov_degrees, install};
use cs_app::input::{DeviceEvent, InputCollector, InputSession, SessionMode};
use cs_app::physics::FlightSpawnSpec;
use cs_content::replay::{BuildId, ChoiceSlot};
use cs_content::settings::{ModernAssist, ProfileKind, Settings};
use cs_sim::control::LocalSeatId;
use cs_sim::flight::synthetic_fixed_wing;
use cs_types::Tick;
use cs_types::content::{ContentId, ContentKind};
use cs_types::input::{
    ActionMap, CommandStream, DeviceClass, DeviceId, FlightCommand, GamepadAxis, InputFrame,
};

fn mouse() -> DeviceId {
    DeviceId::stable(DeviceClass::Mouse, "mouse.f52w3/0").expect("fixture identity")
}

fn pad() -> DeviceId {
    DeviceId::stable(DeviceClass::Gamepad, "pad.f52w3/0").expect("fixture identity")
}

fn settings(profile: ProfileKind, mouse: bool, controller: bool, fov: Option<u16>) -> Settings {
    let mut settings = Settings::designed();
    settings.profile = profile;
    settings.modern.mouse_flight = mouse;
    settings.modern.controller_flight = controller;
    settings.modern.fov_degrees = fov;
    settings
}

fn collector(map: ActionMap) -> InputCollector {
    let mut collector = InputCollector::designed_default(Tick(0));
    collector.bindings_mut().set_map(map);
    for device in [mouse(), pad()] {
        collector.connect_device(device).expect("device connects");
    }
    collector
}

fn frame_of(map: ActionMap, events: &[DeviceEvent]) -> InputFrame {
    let mut collector = collector(map);
    collector.begin_frame(Tick(1));
    for event in events {
        collector.observe_device(event).expect("event applies");
    }
    collector.take_frame()
}

fn mouse_move() -> DeviceEvent {
    DeviceEvent::MouseFrame {
        device: mouse(),
        buttons: vec![],
        motion_x: 0.5,
        motion_y: 0.5,
        wheel: 0.0,
    }
}

fn stick(axis: GamepadAxis) -> DeviceEvent {
    DeviceEvent::GamepadFrame {
        device: pad(),
        buttons: vec![],
        axes: vec![(axis, 0.75)],
    }
}

fn level(frame: &InputFrame, command: FlightCommand) -> f32 {
    frame.axis(command).map_or(0.0, |axis| axis.as_unit())
}

#[test]
fn accept_f52_w3_a_mouse_assist_requested_under_the_original_rules_changes_nothing() {
    let base = ActionMap::designed_default();
    // The option is stored but the profile is the original rules: inert.
    let inert = settings(ProfileKind::OriginalRules, true, true, Some(100)).gameplay_inputs();
    let map = apply_assists(&base, &inert);
    let frame = frame_of(map, &[mouse_move()]);
    assert_eq!(level(&frame, FlightCommand::Yaw), 0.0);
    assert_eq!(level(&frame, FlightCommand::Pitch), 0.0);
    assert_eq!(
        fov_degrees(&inert),
        None,
        "an inert field of view is not applied"
    );

    // The same stored option under the modern profile acts.
    let active = settings(ProfileKind::ModernAssist, true, false, None).gameplay_inputs();
    let frame = frame_of(apply_assists(&base, &active), &[mouse_move()]);
    assert!(level(&frame, FlightCommand::Yaw) > 0.0, "motion X yaws");
    assert!(
        level(&frame, FlightCommand::Pitch) < 0.0,
        "motion Y pitches"
    );
}

#[test]
fn accept_f52_w3_each_assist_changes_only_its_own_mapping() {
    let base = ActionMap::designed_default();
    let none = settings(ProfileKind::ModernAssist, false, false, None).gameplay_inputs();
    let mouse_only = settings(ProfileKind::ModernAssist, true, false, None).gameplay_inputs();
    let pad_only = settings(ProfileKind::ModernAssist, false, true, None).gameplay_inputs();

    // Left stick X rolls in the base layout; the controller assist moves the
    // roll to the right stick and gives the left stick yaw.
    let left = stick(GamepadAxis::LeftStickX);
    let right = stick(GamepadAxis::RightStickX);
    let base_left = frame_of(apply_assists(&base, &none), std::slice::from_ref(&left));
    assert!(level(&base_left, FlightCommand::Roll) > 0.0);
    let modern_left = frame_of(apply_assists(&base, &pad_only), std::slice::from_ref(&left));
    assert_eq!(level(&modern_left, FlightCommand::Roll), 0.0);
    assert!(level(&modern_left, FlightCommand::Yaw) > 0.0);
    let modern_right = frame_of(
        apply_assists(&base, &pad_only),
        std::slice::from_ref(&right),
    );
    assert!(level(&modern_right, FlightCommand::Roll) > 0.0);

    // The mouse assist leaves the sticks as the base layout has them…
    let with_mouse = frame_of(
        apply_assists(&base, &mouse_only),
        std::slice::from_ref(&left),
    );
    assert_eq!(with_mouse, base_left);
    // …and the controller assist leaves the mouse unbound.
    let pad_mouse = frame_of(apply_assists(&base, &pad_only), &[mouse_move()]);
    assert_eq!(level(&pad_mouse, FlightCommand::Yaw), 0.0);

    // Bindings the assists do not own are identical across all three maps.
    let owned = |source: cs_types::input::BindingSource| {
        matches!(
            source,
            cs_types::input::BindingSource::MouseAxis(_)
                | cs_types::input::BindingSource::GamepadAxis(
                    GamepadAxis::LeftStickX
                        | GamepadAxis::LeftStickY
                        | GamepadAxis::RightStickX
                        | GamepadAxis::RightStickY
                )
        )
    };
    let rest = |map: &ActionMap| {
        map.bindings()
            .iter()
            .filter(|b| !owned(b.source))
            .copied()
            .collect::<Vec<_>>()
    };
    assert_eq!(rest(&apply_assists(&base, &mouse_only)), rest(&base));
    assert_eq!(rest(&apply_assists(&base, &pad_only)), rest(&base));
}

#[test]
fn accept_f52_w3_install_changes_the_live_session_map() {
    let base = ActionMap::designed_default();
    let mut session =
        InputSession::designed_default(LocalSeatId(0), SessionMode::SinglePlayer, Tick(0));
    install(
        &mut session,
        &base,
        &settings(ProfileKind::OriginalRules, true, false, None).gameplay_inputs(),
    );
    assert_eq!(
        session
            .bindings()
            .map()
            .target_for(cs_types::input::BindingSource::MouseAxis(
                cs_types::input::MouseAxis::X
            )),
        None,
        "the original rules leave mouse motion unbound"
    );
    install(
        &mut session,
        &base,
        &settings(ProfileKind::ModernAssist, true, false, None).gameplay_inputs(),
    );
    assert!(
        session
            .bindings()
            .map()
            .target_for(cs_types::input::BindingSource::MouseAxis(
                cs_types::input::MouseAxis::X
            ))
            .is_some()
    );
}

fn record_with(settings: &Settings) -> cs_content::replay::ReplayRecord {
    let tree = BuildId::new("0f1e2d3c4b5a69788796a5b4c3d2e1f001234567").expect("tree hash");
    let build = BuildContext::on_host(tree, "rustc test");
    let id = ContentId::from_source(ContentKind::Mission, "synthetic.f52-w3").expect("id");
    let subject = ReplaySubject::flight(
        id,
        synthetic_fixed_wing(),
        FlightSpawnSpec::level_at([0.0, 1000.0, 0.0], [0.0, 0.0, -120.0]),
    )
    .with_fidelity(&settings.fidelity());
    let stream = CommandStream::new();
    record(&RunRequest {
        subject: &subject,
        build: &build,
        stream: &stream,
        ticks: 2,
    })
    .expect("the fixture run records")
}

#[test]
fn accept_f52_w3_a_labelled_and_an_unlabelled_run_differ_in_the_record_alone() {
    let plain = record_with(&settings(ProfileKind::OriginalRules, true, true, Some(90)));
    let labelled = record_with(&settings(ProfileKind::ModernAssist, true, false, Some(100)));

    assert_eq!(
        plain.choices.get(ChoiceSlot::Assists),
        Some("original-rules")
    );
    assert_eq!(
        labelled.choices.get(ChoiceSlot::Assists),
        Some("mouse-flight,fov-100")
    );
    assert_ne!(plain.choices.canonical(), labelled.choices.canonical());
    let _ = ModernAssist::MouseFlight;
}
