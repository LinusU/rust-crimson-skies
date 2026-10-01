use cs_app::accessibility::motion::{Effect, filter_effects};
use cs_content::audio::{AudioBus, AudioLevel};
use cs_content::settings::{
    ColourFilter, ModernAssist, ProfileKind, Resolution, Settings, SettingsError,
};

fn all_presentation_changed() -> Settings {
    let mut s = Settings::designed();
    let p = &mut s.presentation;
    p.ui_scale_percent = 250;
    p.subtitles = true;
    p.colour_filter = ColourFilter::Monochrome;
    p.bus_levels
        .insert(AudioBus::Radio, AudioLevel::try_new(0.25).unwrap());
    p.reduce_shake = true;
    p.reduce_flash = true;
    p.resolution = Resolution {
        width: 1920,
        height: 1080,
    };
    s
}

#[test]
fn accept_f52_a_presentation_settings_leave_gameplay_inputs_and_metadata_unchanged() {
    let plain = Settings::designed();
    let changed = all_presentation_changed();
    assert_ne!(plain, changed);
    assert_eq!(plain.gameplay_inputs(), changed.gameplay_inputs());
    assert_eq!(plain.fidelity().metadata(), changed.fidelity().metadata());
    assert!(changed.fidelity().is_original_rules());
}

#[test]
fn accept_f52_a_reduced_motion_drops_cosmetics_and_keeps_required_notices() {
    let effects = [
        Effect::CameraShake(0.5),
        Effect::DamageNotice,
        Effect::ScreenFlash(1.0),
        Effect::TargetNotice,
    ];
    let reduced = all_presentation_changed().presentation;
    assert_eq!(
        filter_effects(&reduced, &effects),
        vec![Effect::DamageNotice, Effect::TargetNotice]
    );
    let plain = Settings::designed().presentation;
    assert_eq!(filter_effects(&plain, &effects), effects.to_vec());
    // Each switch is independent.
    let mut shake_only = Settings::designed().presentation;
    shake_only.reduce_shake = true;
    assert_eq!(
        filter_effects(&shake_only, &effects),
        vec![
            Effect::DamageNotice,
            Effect::ScreenFlash(1.0),
            Effect::TargetNotice
        ]
    );
    assert!(effects.iter().filter(|e| e.is_required()).count() == 2);
}

#[test]
fn accept_f52_a_an_enabled_assist_is_named_in_the_fidelity_metadata() {
    let mut s = Settings::designed();
    s.modern.mouse_flight = true;
    s.modern.fov_degrees = Some(100);
    // Inert under the original rules: recorded as original.
    assert_eq!(
        s.fidelity().metadata(),
        vec![("fidelity".to_owned(), "original-rules".to_owned())]
    );
    assert!(s.gameplay_inputs().assists.is_empty());

    s.profile = ProfileKind::ModernAssist;
    assert_eq!(
        s.gameplay_inputs().assists,
        vec![ModernAssist::MouseFlight, ModernAssist::Fov(100)]
    );
    assert_eq!(
        s.fidelity().metadata(),
        vec![
            ("fidelity".to_owned(), "modified".to_owned()),
            ("assists".to_owned(), "mouse-flight,fov-100".to_owned()),
        ]
    );
    // The modern profile with nothing turned on is not a modification.
    s.modern = Default::default();
    assert!(s.fidelity().is_original_rules());
}

#[test]
fn accept_f52_a_settings_round_trip_through_the_strict_text_form() {
    let mut s = all_presentation_changed();
    s.profile = ProfileKind::ModernAssist;
    s.modern.controller_flight = true;
    s.modern.fov_degrees = Some(90);
    assert_eq!(Settings::from_text(&s.to_text()).expect("parse"), s);
    let d = Settings::designed();
    assert_eq!(Settings::from_text(&d.to_text()).expect("parse"), d);
}

#[test]
fn accept_f52_a_the_text_form_refuses_what_it_does_not_understand() {
    let good = Settings::designed().to_text();
    let err = |text: String| Settings::from_text(&text).expect_err("refused");
    assert_eq!(
        err(good.replacen("cs-settings 1", "cs-settings 2", 1)),
        SettingsError::BadHeader
    );
    assert!(matches!(
        err(format!("{good}extra=1\n")),
        SettingsError::UnknownKey(key) if key == "extra"
    ));
    assert!(matches!(
        err(format!("{good}width=800\n")),
        SettingsError::DuplicateKey(key) if key == "width"
    ));
    assert!(matches!(
        err(good.replace("subtitles=false\n", "")),
        SettingsError::MissingKey("subtitles")
    ));
    assert!(matches!(
        err(good.replace("ui_scale_percent=100", "ui_scale_percent=900")),
        SettingsError::UiScale(900)
    ));
    assert!(matches!(
        err(good.replace("width=1280", "width=10")),
        SettingsError::Resolution { .. }
    ));
    assert!(matches!(
        err(good.replace("fov_degrees=none", "fov_degrees=300")),
        SettingsError::Fov(300)
    ));
    assert!(matches!(
        err(good.replace("bus.radio=1", "bus.radio=-1")),
        SettingsError::Level(_)
    ));
}
