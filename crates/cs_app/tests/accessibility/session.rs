//! Acceptance stage F52-C: safe recovery, the labelled control profile and
//! one projection to every consumer
//! (`specs/F52-accessibility-and-explicitly-separated-modern-options.md`,
//! section `### F52-C`).
//!
//! The minimum scenario is AC03's: turn shake and flash off and verify the
//! gameplay telemetry is unchanged. Everything runs through the production
//! `SettingsSession` — its file I/O, its apply/retry/teardown and its
//! consumers — and the production flight telemetry of
//! `airframe_visual::MissionAircraftSession`; nothing here is a parallel,
//! test-only implementation. The fixtures are synthetic, so none of it is
//! evidence about an original option.

use std::fs;
use std::path::{Path, PathBuf};

use cs_app::accessibility::motion::Effect;
use cs_app::accessibility::session::{ApplyError, SettingsSession};
use cs_app::accessibility::store::{LoadError, StartupOrigin, load, save_atomic};
use cs_app::airframe_visual::MissionAircraftSession;
use cs_app::input::SessionMode;
use cs_app::objectives::{ObjectiveSession, lower_program};
use cs_app::ui::hud::{AircraftSample, HudSession, MissionPage, MissionSources, PageView};
use cs_content::airframe_roles::{ForcedAssignment, OwnedLoadout, declared_synthetic_roles};
use cs_content::hud::HudPolicy;
use cs_content::objectives::declared_synthetic_objectives;
use cs_content::settings::{
    ColourFilter, GameplayInputs, MAX_UI_SCALE_PERCENT, ModernAssist, ProfileKind, Settings,
    SettingsError,
};
use cs_script::runtime::SessionGeneration;
use cs_sim::flight::{
    EngineState, FlightEnvironment, FlightInput, FlightState, FlightTelemetry, SYNTHETIC_TICK_DT_S,
    TelemetryFrame, synthetic_exceptional_profile, synthetic_exceptional_tuning,
};
use cs_types::Tick;
use cs_types::content::{ContentId, ContentKind, Origin, Provenance};
use cs_types::evidence::ClaimId;
use cs_types::net::{ActorId, SessionId};
use cs_types::space::Quaternion;

struct Dir(PathBuf);

impl Dir {
    fn new(name: &str) -> Self {
        let root = std::env::temp_dir().join(format!(
            "cs-f52-c-{name}-{}-{:?}",
            std::process::id(),
            std::thread::current().id()
        ));
        let _ = fs::remove_dir_all(&root);
        fs::create_dir_all(&root).expect("dir");
        Self(root)
    }

    fn file(&self, name: &str) -> PathBuf {
        self.0.join(name)
    }
}

impl Drop for Dir {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

/// The sibling temporary file every save writes first.
fn temporary(path: &Path) -> PathBuf {
    let mut name = path.file_name().expect("a file name").to_os_string();
    name.push(".tmp");
    path.with_file_name(name)
}

/// Blocks every save by taking the temporary name with a directory.
fn block_saves(path: &Path) {
    fs::create_dir(temporary(path)).expect("block the save");
}

fn unblock_saves(path: &Path) {
    fs::remove_dir(temporary(path)).expect("release the save");
}

/// One measured frame of gameplay: the production mission-aircraft telemetry
/// plus the assists the session's own gameplay view reported for the frame.
#[derive(Debug, PartialEq)]
struct Sample {
    telemetry: TelemetryFrame,
    assists: Vec<ModernAssist>,
}

/// Flies the production exceptional control law for 50 fixed ticks and reads
/// the telemetry back out of it, the way the F25-C mission session does.
fn flight_sample(inputs: &GameplayInputs) -> Sample {
    let roles = declared_synthetic_roles();
    let airframe = |key: &str| {
        ContentId::from_source(ContentKind::Airframe, key).expect("a valid airframe id")
    };
    let owned = OwnedLoadout {
        airframe: airframe("fixture.synthetic-fixed-wing"),
        session_generation: 7,
    };
    let forced = ForcedAssignment {
        airframe: airframe("fixture.synthetic-autogyro"),
        session_generation: 7,
        origin: Origin::SyntheticFixture,
        provenance: Provenance::designed(ClaimId::new("f52c.test.forced").expect("claim id")),
    };
    let mut session = MissionAircraftSession::launch(
        &roles,
        &owned,
        Some(&forced),
        synthetic_exceptional_tuning(),
        synthetic_exceptional_profile(),
    )
    .expect("the synthetic exceptional launch resolves");
    let environment = FlightEnvironment::SEA_LEVEL;
    let state = FlightState {
        linear_velocity_mps: [0.0, 0.0, -20.0],
        engine: EngineState::direct(0.8),
        ..FlightState::at_rest(Quaternion::IDENTITY)
    };
    let input = FlightInput::try_new(0.3, 0.2, 0.0, 0.8, false).expect("legal input");
    let mut last = None;
    for tick in 1..=50_u64 {
        let produced = session
            .fly(
                &environment,
                &state,
                &input,
                SYNTHETIC_TICK_DT_S,
                Tick(tick),
            )
            .expect("a legal tick");
        last = Some(
            session
                .telemetry(&produced, &state, Tick(tick))
                .expect("telemetry"),
        );
    }
    Sample {
        telemetry: last.expect("50 ticks flew"),
        assists: inputs.assists.clone(),
    }
}

/// AC03's minimum scenario: with shake and flash off, the frame the player
/// sees changes and the gameplay telemetry does not.
#[test]
fn accept_f52_c_turning_off_shake_and_flash_leaves_gameplay_telemetry_unchanged() {
    let dir = Dir::new("ac03");
    let plain = SettingsSession::open(dir.file("plain.txt"), false);
    let mut reduced = SettingsSession::open(dir.file("reduced.txt"), false);
    reduced
        .apply(|settings| {
            settings.presentation.reduce_shake = true;
            settings.presentation.reduce_flash = true;
        })
        .expect("the change is valid and the file is writable");

    let effects = [
        Effect::CameraShake(0.5),
        Effect::ScreenFlash(1.0),
        Effect::DamageNotice,
        Effect::TargetNotice,
    ];
    let seen_by_plain = plain.project(&effects, flight_sample);
    let seen_by_reduced = reduced.project(&effects, flight_sample);

    // The two sessions really differ, on disk as well as in memory.
    assert_ne!(
        plain.settings().presentation,
        reduced.settings().presentation
    );
    assert!(
        fs::read_to_string(reduced.path())
            .expect("the change was saved")
            .contains("reduce_shake=true"),
        "the difference is on disk, not only in memory"
    );
    assert!(
        !plain.path().exists(),
        "opening a session reads the file, it never writes it"
    );

    // AC03: the gameplay telemetry is unchanged.
    assert_eq!(seen_by_plain.gameplay, seen_by_reduced.gameplay);
    assert!(
        seen_by_plain.gameplay.telemetry.shared().airspeed_mps > 0.0,
        "the telemetry is a real measurement, not a default"
    );
    // ...and so is every view the simulation and the records read.
    assert_eq!(plain.gameplay_inputs(), reduced.gameplay_inputs());
    assert_eq!(seen_by_plain.fidelity, seen_by_reduced.fidelity);
    assert_eq!(
        plain.control_profile().label.metadata(),
        reduced.control_profile().label.metadata()
    );

    // While the presentation the player sees really changes: the cosmetics
    // are dropped and the required notifications pass, in order.
    assert_eq!(seen_by_plain.presented, effects.to_vec());
    assert_eq!(
        seen_by_reduced.presented,
        vec![Effect::DamageNotice, Effect::TargetNotice]
    );

    // Each switch governs only its own effect.
    let mut shake_only = SettingsSession::open(dir.file("shake.txt"), false);
    shake_only
        .apply(|settings| settings.presentation.reduce_shake = true)
        .expect("saved");
    assert_eq!(
        shake_only.present(&effects),
        vec![
            Effect::ScreenFlash(1.0),
            Effect::DamageNotice,
            Effect::TargetNotice
        ],
        "only the shake is dropped, and the order is the frame's own"
    );
}

/// Non-negotiable behavior 5: an unusable file recovers with the reason
/// reported and the file left as it was, the safe flag skips reading it, and
/// neither path rewrites anything on its own.
#[test]
fn accept_f52_c_a_session_recovers_from_an_unusable_file_and_reports_why() {
    let dir = Dir::new("recovery");
    let path = dir.file("settings.txt");

    let fresh = SettingsSession::open(&path, false);
    assert!(matches!(fresh.origin(), StartupOrigin::NoFile));
    assert!(fresh.recovery().is_none());
    assert_eq!(fresh.settings(), &Settings::designed());

    // A display configuration no window could use.
    let unusable = Settings::designed()
        .to_text()
        .replace("width=1280", "width=0");
    fs::write(&path, &unusable).expect("write");
    let mut recovered = SettingsSession::open(&path, false);
    assert!(matches!(
        recovered.origin(),
        StartupOrigin::Recovered(LoadError::Invalid(_))
    ));
    assert!(
        matches!(recovered.recovery(), Some(LoadError::Invalid(_))),
        "the reason is kept for the screen that reports it"
    );
    assert_eq!(recovered.settings(), &Settings::designed());
    assert_eq!(
        fs::read_to_string(&path).expect("read"),
        unusable,
        "recovery never rewrites the file it could not use"
    );

    // Only a real change replaces it, atomically.
    recovered
        .apply(|settings| settings.presentation.subtitles = true)
        .expect("the save works now");
    assert_eq!(load(&path).expect("valid again"), *recovered.settings());
    assert!(!temporary(&path).exists());

    // The safe flag does not even read it.
    let safe = SettingsSession::open(&path, true);
    assert!(matches!(safe.origin(), StartupOrigin::SafeFlag));
    assert!(safe.recovery().is_none());
    assert_eq!(safe.settings(), &Settings::designed());
    assert_eq!(
        load(&path).expect("kept"),
        *recovered.settings(),
        "the flag leaves the saved settings alone"
    );
}

/// A change that is not valid settings is refused before anything moves: the
/// session, the file and every consumer stay exactly as they were.
#[test]
fn accept_f52_c_a_refused_change_is_reported_and_never_reaches_the_file() {
    let dir = Dir::new("refused");
    let path = dir.file("settings.txt");
    save_atomic(&path, &Settings::designed()).expect("seed the file");
    let before = fs::read(&path).expect("read");

    let mut session = SettingsSession::open(&path, false);
    let effects = [Effect::CameraShake(0.9), Effect::DamageNotice];
    let refused = session
        .apply(|settings| settings.presentation.ui_scale_percent = 900)
        .expect_err("900 % is not a usable UI scale");
    assert!(
        matches!(refused, ApplyError::Invalid(SettingsError::UiScale(900))),
        "the refusal names the value it rejected: {refused}"
    );
    assert!(!session.is_staged());
    assert_eq!(session.settings().presentation.ui_scale_percent, 100);
    assert_eq!(session.present(&effects), effects.to_vec());
    assert_eq!(fs::read(&path).expect("read"), before);

    // A later, valid change still works: nothing was poisoned by the refusal.
    session
        .apply(|settings| settings.presentation.reduce_flash = true)
        .expect("a valid change is accepted");
    assert!(!session.is_staged());
    assert_eq!(load(&path).expect("saved"), *session.settings());
}

/// A change that cannot be written is kept, reported and retried — never
/// dropped, never half-written.
#[test]
fn accept_f52_c_a_failed_save_stays_staged_and_retries_until_it_lands() {
    let dir = Dir::new("retry");
    let path = dir.file("settings.txt");
    let mut session = SettingsSession::open(&path, false);
    session
        .apply(|settings| settings.presentation.subtitles = true)
        .expect("the first save");
    let before = fs::read(&path).expect("read");

    block_saves(&path);
    let refused = session
        .apply(|settings| settings.presentation.reduce_shake = true)
        .expect_err("the save cannot start");
    assert!(matches!(refused, ApplyError::Persist(_)));
    assert!(
        session.is_staged(),
        "the change is live here and waiting for the file"
    );
    assert!(session.settings().presentation.reduce_shake);
    assert_eq!(
        fs::read(&path).expect("read"),
        before,
        "a failed save leaves the previous file whole"
    );
    assert!(
        matches!(session.retry(), Err(ApplyError::Persist(_))),
        "a retry reports the same cause while it lasts"
    );
    assert!(session.is_staged(), "a failed retry drops nothing");

    unblock_saves(&path);
    session.retry().expect("the retry lands");
    assert!(!session.is_staged());
    assert_eq!(
        load(&path).expect("saved"),
        *session.settings(),
        "the whole staged change is on disk, in one write"
    );
}

/// Teardown flushes a staged change or reports it and stays open, so a change
/// is never lost on the way out; only a successful teardown closes the
/// session.
#[test]
fn accept_f52_c_teardown_flushes_a_staged_change_or_reports_it_and_stays_open() {
    let dir = Dir::new("teardown");
    let path = dir.file("settings.txt");
    let mut session = SettingsSession::open(&path, false);
    session
        .apply(|settings| settings.presentation.reduce_flash = true)
        .expect("the first save");

    block_saves(&path);
    session
        .apply(|settings| settings.presentation.subtitles = true)
        .expect_err("the save cannot start");
    let refused = session.teardown().expect_err("nothing can be flushed");
    assert!(matches!(refused, ApplyError::Persist(_)));
    assert!(
        !session.is_closed(),
        "a failed teardown keeps the session open so the caller can retry"
    );
    assert!(session.is_staged());

    unblock_saves(&path);
    session.teardown().expect("the flush lands");
    assert!(session.is_closed());
    assert!(!session.is_staged());
    assert_eq!(
        load(&path).expect("saved"),
        *session.settings(),
        "teardown wrote the staged change before closing"
    );

    // A closed session is read-only, and closing twice is not an error.
    let closed = session
        .apply(|settings| settings.presentation.subtitles = false)
        .expect_err("a closed session accepts no change");
    assert!(matches!(closed, ApplyError::Closed));
    assert!(matches!(session.retry(), Err(ApplyError::Closed)));
    session
        .teardown()
        .expect("tearing down twice is not an error");
}

/// AC04's data half through the live session: a stored modern option is inert
/// under the original rules, and it moves the gameplay inputs and the label
/// together only when the modern profile is on — and both survive a restart.
#[test]
fn accept_f52_c_the_control_profile_and_its_label_move_together_only_for_a_real_assist() {
    let dir = Dir::new("label");
    let path = dir.file("settings.txt");
    let mut session = SettingsSession::open(&path, false);

    assert_eq!(session.control_profile().kind, ProfileKind::OriginalRules);
    assert!(session.control_profile().label.is_original_rules());
    assert!(session.gameplay_inputs().assists.is_empty());

    // Storing a modern option under the original rules changes nothing a
    // record or the simulation can see.
    session
        .apply(|settings| settings.modern.mouse_flight = true)
        .expect("saved");
    assert!(session.gameplay_inputs().assists.is_empty());
    assert!(session.control_profile().label.is_original_rules());

    // Switching the profile on names the assist in both views at once.
    session
        .apply(|settings| settings.profile = ProfileKind::ModernAssist)
        .expect("saved");
    let profile = session.control_profile();
    assert_eq!(profile.kind, ProfileKind::ModernAssist);
    assert_eq!(profile.assists, vec![ModernAssist::MouseFlight]);
    assert_eq!(profile.label.assists, profile.assists);
    assert_eq!(
        session.fidelity().metadata(),
        vec![
            ("fidelity".to_owned(), "modified".to_owned()),
            ("assists".to_owned(), "mouse-flight".to_owned()),
        ]
    );

    // The label is persisted with the profile it belongs to.
    let reopened = SettingsSession::open(&path, false);
    assert_eq!(reopened.control_profile(), profile);
    assert_eq!(reopened.gameplay_inputs(), session.gameplay_inputs());
    assert!(!reopened.is_staged());

    // Switching back to the original rules leaves the stored option where it
    // is but takes it out of force: the label says original-rules again.
    session
        .apply(|settings| settings.profile = ProfileKind::OriginalRules)
        .expect("saved");
    assert!(session.control_profile().label.is_original_rules());
    assert!(session.gameplay_inputs().assists.is_empty());
    assert!(session.settings().modern.mouse_flight, "nothing was lost");
}

/// The objectives page is read through the live session: the player's own
/// presentation change reaches the page on the next read, through the real
/// `HudSession` producer.
#[test]
fn accept_f52_c_the_objectives_page_is_read_through_the_sessions_presentation() {
    let dir = Dir::new("page");
    let mut session = SettingsSession::open(dir.file("settings.txt"), false);

    let objectives = ObjectiveSession::launch(
        lower_program(&declared_synthetic_objectives()).expect("the synthetic program lowers"),
        SessionGeneration(1),
    )
    .expect("the objective session launches");
    let id = SessionId::new(7).expect("a session id");
    let actor = ActorId {
        session: id,
        serial: 1,
    };
    let mut hud =
        HudSession::new(SessionMode::SinglePlayer, HudPolicy::designed()).expect("the HUD session");
    hud.bind(id, actor).expect("bound");
    hud.open(MissionPage::Objectives);
    let sample = AircraftSample {
        session: id,
        actor,
        attitude: Quaternion::IDENTITY,
        velocity_mps: [0.0, 0.0, -50.0],
        wind_mps: [0.0; 3],
        height_m: 500.0,
        ground_height_m: Some(0.0),
    };
    let view = hud
        .view(
            &sample,
            &MissionSources {
                objectives: Some(objectives.display()),
                ..MissionSources::default()
            },
        )
        .expect("the objectives view");

    let designed = session
        .page(&view, 1_000)
        .expect("the objectives page at the designed scale");
    assert_eq!(designed.lines.len(), 1);
    assert_eq!(designed.metrics.text_px, 14, "the designed 100 % height");
    assert!(designed.lines[0].cue.colour.is_some(), "the filter is off");

    // The player changes the settings; the very next read through the
    // session shows them, with no session reopened.
    session
        .apply(|settings| {
            settings.presentation.ui_scale_percent = MAX_UI_SCALE_PERCENT;
            settings.presentation.colour_filter = ColourFilter::Monochrome;
        })
        .expect("saved");
    let scaled = session
        .page(&view, 1_000)
        .expect("the same view, read again");
    assert_eq!(scaled.lines.len(), designed.lines.len());
    assert_eq!(
        scaled.metrics.text_px,
        designed.metrics.text_px * u32::from(MAX_UI_SCALE_PERCENT) / 100
    );
    assert_eq!(scaled.lines[0].cue.colour, None);
    assert_eq!(
        scaled.lines[0].cue.shape, designed.lines[0].cue.shape,
        "the filter changes only the redundant colour role"
    );

    // A view that is not the objectives page still produces no page.
    hud.open(MissionPage::Recon);
    let other = hud
        .view(&sample, &MissionSources::default())
        .expect("the recon view");
    assert!(!matches!(other, PageView::Objectives(_)));
    assert!(session.page(&other, 1_000).is_none());
}
