//! Acceptance stage F52-D: AC04 — a gameplay assist is enabled and the
//! comparison/replay metadata records it
//! (`specs/F52-accessibility-and-explicitly-separated-modern-options.md`,
//! section `### F52-D`). Shared contracts: `docs/contracts/UI-NETWORK.md` and
//! `docs/contracts/CLI-EVIDENCE.md`.
//!
//! Everything runs through the production [`SettingsSession`] — its file I/O,
//! its apply path and its consumers — and through `cs_content::replay`'s own
//! record types; nothing here is a parallel, test-only implementation of the
//! label. The fixtures are synthetic, so none of it is evidence about an
//! original option.
//!
//! What is **not** claimed: no writer in `crates/cs_app/src/capture/` puts
//! this label into a `ReplayRecord` yet — that wiring is F52-W3's (#782), whose
//! owner paths are the input/flight and capture/replay writer. The record form
//! below is built from the session's own label to show it survives into the
//! document a replay carries, and the finding
//! (`docs/findings/2026-10-08-f52-d-accessibility-flows-gpu-review.md`) keeps
//! that gap open by name.

use std::fs;
use std::path::PathBuf;

use cs_app::accessibility::session::{ApplyError, SettingsSession};
use cs_app::accessibility::store;
use cs_content::replay::{AuthoredChoice, AuthoredChoices, ChoiceSlot};
use cs_content::settings::{
    ColourFilter, MAX_UI_SCALE_PERCENT, ModernAssist, ProfileKind, Settings,
};
use cs_types::content::Provenance;
use cs_types::evidence::ClaimId;

/// A throwaway settings directory that dies with the test.
struct Dir(PathBuf);

impl Dir {
    fn new(name: &str) -> Self {
        let root = std::env::temp_dir().join(format!(
            "cs-f52-d-{name}-{}-{:?}",
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

/// The `("assists", ...)` entry of a label, i.e. exactly what a record names.
fn assists_value(label: &[(String, String)]) -> String {
    label
        .iter()
        .find(|(key, _)| key == "assists")
        .map(|(_, value)| value.clone())
        .expect("a modified run names its assists")
}

/// AC04's minimum scenario: turn a gameplay assist on in the live session and
/// the record names it — in the frame's fidelity metadata, in the labelled
/// control profile, and in the authored choice a replay document pins — while
/// an original-rules run stays `original-rules`.
#[test]
fn accept_f52_d_enabling_a_gameplay_assist_is_named_in_the_comparison_and_replay_metadata() {
    let dir = Dir::new("assist");
    let path = dir.file("settings.txt");
    let mut session = SettingsSession::open(&path, false);
    assert!(
        session.fidelity().is_original_rules(),
        "a fresh session is the original rules: the record says so before anything is enabled"
    );
    assert_eq!(
        session.fidelity().metadata(),
        vec![("fidelity".to_owned(), "original-rules".to_owned())],
        "an unmodified run records itself as one"
    );

    session
        .apply(|settings| {
            settings.profile = ProfileKind::ModernAssist;
            settings.modern.mouse_flight = true;
            settings.modern.fov_degrees = Some(100);
        })
        .expect("the change validates and is persisted");
    assert!(!session.is_staged(), "the record's own file is up to date");

    let expected = vec![
        ("fidelity".to_owned(), "modified".to_owned()),
        ("assists".to_owned(), "mouse-flight,fov-100".to_owned()),
    ];
    // The metadata itself — what comparison and replay records carry (AC04).
    assert_eq!(session.fidelity().metadata(), expected);
    // …read again through the one-reading projection, so the frame's record
    // and the gameplay it was measured under cannot disagree.
    let frame = session.project(&[], |inputs| inputs.assists.clone());
    assert_eq!(frame.fidelity, expected);
    assert_eq!(
        frame.gameplay,
        vec![ModernAssist::MouseFlight, ModernAssist::Fov(100)]
    );
    // …and through the labelled control profile a consumer acts on.
    let profile = session.control_profile();
    assert_eq!(profile.kind, ProfileKind::ModernAssist);
    assert_eq!(
        profile.assists,
        vec![ModernAssist::MouseFlight, ModernAssist::Fov(100)]
    );
    assert_eq!(profile.label.metadata(), expected);

    // The same record is read back from the file a later comparison opens:
    // the persisted settings name the assist, in the text form and in the
    // label built from it.
    let on_disk = store::load(&path).expect("the settings file is readable");
    assert_eq!(on_disk.fidelity().metadata(), expected);
    let text = fs::read_to_string(&path).expect("the settings file exists");
    assert!(text.contains("profile=modern-assist\n"));
    assert!(text.contains("mouse_flight=true\n"));
    let reopened = SettingsSession::open(&path, false);
    assert_eq!(reopened.fidelity().metadata(), expected);
    assert_eq!(reopened.control_profile().label.metadata(), expected);

    // The record *form* a replay document carries: the assists string is the
    // label's own, pinned under the slot that means "which assists were
    // enabled", so a labelled run and an unmodified run are distinguishable
    // from the document alone. (The production writer of that choice is
    // F52-W3's, #782 — see the F52-D finding.)
    let claim = ClaimId::new("f52d.record-test").expect("claim id");
    let pinned = AuthoredChoices::new(vec![AuthoredChoice {
        slot: ChoiceSlot::Assists,
        value: assists_value(&session.fidelity().metadata()),
        provenance: Provenance::designed(claim),
    }])
    .expect("the assisted run pins its assists");
    assert_eq!(
        pinned.get(ChoiceSlot::Assists),
        Some("mouse-flight,fov-100")
    );

    session
        .apply(|settings| {
            settings.profile = ProfileKind::OriginalRules;
            settings.modern = Default::default();
        })
        .expect("turning the assist off persists");
    assert_eq!(
        session.fidelity().metadata(),
        vec![("fidelity".to_owned(), "original-rules".to_owned())],
        "the record of the unmodified run names no assist, so the two runs are distinguishable"
    );
    assert!(
        pinned.canonical().contains("mouse-flight,fov-100"),
        "the document text carries the label's own assist names: {}",
        pinned.canonical()
    );
}

/// An assist that is not in force must never be recorded as a modification:
/// the stored modern options are inert under the original rules, a refused
/// change never reaches the record, and presentation settings are not assists
/// (AC03's data half).
#[test]
fn accept_f52_d_an_inert_assist_and_a_refused_change_never_reach_the_record() {
    let dir = Dir::new("inert");
    let path = dir.file("settings.txt");
    let mut session = SettingsSession::open(&path, false);

    // The modern options are configured but the profile is the original rules:
    // nothing is in force, nothing is recorded.
    session
        .apply(|settings| {
            settings.modern.mouse_flight = true;
            settings.modern.controller_flight = true;
            settings.modern.fov_degrees = Some(90);
        })
        .expect("configuring the options persists");
    assert!(session.fidelity().is_original_rules());
    assert!(session.gameplay_inputs().assists.is_empty());
    assert!(session.control_profile().assists.is_empty());
    assert_eq!(
        session.fidelity().metadata(),
        vec![("fidelity".to_owned(), "original-rules".to_owned())],
        "an inert assist is not a modification"
    );

    // A refused change leaves the record exactly as it was.
    let before = session.fidelity().metadata();
    let error = session
        .apply(|settings| settings.modern.fov_degrees = Some(300))
        .expect_err("an out-of-range field of view is refused");
    assert!(matches!(error, ApplyError::Invalid(_)));
    assert_eq!(session.fidelity().metadata(), before);
    assert_eq!(
        store::load(&path)
            .expect("the file is unchanged")
            .fidelity()
            .metadata(),
        before
    );

    // Presentation settings are not gameplay assists: they never change the
    // record, whatever they are set to.
    session
        .apply(|settings| {
            settings.profile = ProfileKind::ModernAssist;
            settings.presentation.colour_filter = ColourFilter::Monochrome;
            settings.presentation.ui_scale_percent = MAX_UI_SCALE_PERCENT;
            settings.presentation.reduce_shake = true;
            settings.presentation.reduce_flash = true;
            settings.presentation.subtitles = true;
        })
        .expect("the presentation change persists");
    let with_assist = session.fidelity().metadata();
    assert_eq!(
        with_assist,
        vec![
            ("fidelity".to_owned(), "modified".to_owned()),
            (
                "assists".to_owned(),
                "mouse-flight,controller-flight,fov-90".to_owned()
            ),
        ],
        "every assist in force is named"
    );
    session
        .apply(|settings| {
            settings.presentation.colour_filter = ColourFilter::Off;
            settings.presentation.ui_scale_percent = 100;
            settings.presentation.reduce_shake = false;
            settings.presentation.reduce_flash = false;
            settings.presentation.subtitles = false;
        })
        .expect("the presentation change persists");
    assert_eq!(
        session.fidelity().metadata(),
        with_assist,
        "presentation settings are not assists and never enter the record"
    );
    assert_eq!(
        Settings::designed().fidelity().metadata(),
        vec![("fidelity".to_owned(), "original-rules".to_owned())],
        "and the designed settings are the original rules"
    );
}

/// A change that cannot be written is still what the run was doing, so the
/// record names it and `is_staged()` says the file is behind — the label is
/// never quietly dropped and never claimed to be saved (non-negotiable 5).
#[test]
fn accept_f52_d_a_change_that_cannot_be_saved_is_recorded_and_says_it_is_not_on_disk() {
    let dir = Dir::new("staged");
    // A directory cannot be replaced by a file, so the atomic rename fails
    // after the change validated: the real failure path, not a mock.
    let path = dir.file("unwritable");
    fs::create_dir(&path).expect("the path is a directory");
    let mut session = SettingsSession::open(&path, false);
    assert!(
        session.recovery().is_some(),
        "the unusable file is reported, not swallowed"
    );

    let error = session
        .apply(|settings| {
            settings.profile = ProfileKind::ModernAssist;
            settings.modern.mouse_flight = true;
        })
        .expect_err("the atomic write cannot replace a directory");
    assert!(matches!(error, ApplyError::Persist(_)));
    assert!(session.is_staged(), "the change is live but not on disk");
    assert_eq!(
        session.fidelity().metadata(),
        vec![
            ("fidelity".to_owned(), "modified".to_owned()),
            ("assists".to_owned(), "mouse-flight".to_owned()),
        ],
        "the record names what was actually in force"
    );
    assert!(path.is_dir(), "the unwritable path is untouched");

    let error = session.teardown().expect_err("the flush fails again");
    assert!(matches!(error, ApplyError::Persist(_)));
    assert!(
        !session.is_closed(),
        "a failed teardown leaves the session open so the caller can retry"
    );
    assert_eq!(
        session.fidelity().metadata(),
        vec![
            ("fidelity".to_owned(), "modified".to_owned()),
            ("assists".to_owned(), "mouse-flight".to_owned()),
        ],
        "and the record still says what was in force"
    );
}
