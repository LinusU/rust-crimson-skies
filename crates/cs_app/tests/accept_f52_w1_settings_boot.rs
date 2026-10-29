//! F52-W1: the application opens the accessibility settings session at boot,
//! and `--safe-settings` requests the safe-defaults startup.
//!
//! Everything here goes through the production parser, the production session
//! opener and the production report line in `cs_app::cli`, and the settings
//! file path of `cs_app::user_data`.

use std::fs;
use std::path::PathBuf;

use cs_app::accessibility::session::SettingsSession;
use cs_app::cli::{self, CliRequest};
use cs_app::user_data::{self, Environment, Platform};

fn args(list: &[&str]) -> Vec<String> {
    list.iter().map(ToString::to_string).collect()
}

fn scratch_dir(name: &str) -> PathBuf {
    let dir = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../../target")
        .join(format!("{name}_{}", std::process::id()));
    if dir.exists() {
        fs::remove_dir_all(&dir).expect("the scratch dir must be reusable");
    }
    fs::create_dir_all(&dir).expect("the scratch dir must be creatable");
    dir
}

fn open(safe: bool, base: &std::path::Path) -> SettingsSession {
    cli::open_accessibility_session(safe, Some(base), Platform::Linux, &Environment::default())
        .expect("a given profile base needs no environment")
}

#[test]
fn accept_f52_w1_safe_settings_parses_with_playtest_only() {
    match cli::parse(args(&["--playtest", "--safe-settings"])) {
        CliRequest::Playtest(request) => assert!(request.safe_settings),
        other => panic!("{other:?}"),
    }
    match cli::parse(args(&["--playtest"])) {
        CliRequest::Playtest(request) => assert!(!request.safe_settings),
        other => panic!("{other:?}"),
    }
    for refused in [
        &["--safe-settings"][..],
        &[
            "--synthetic",
            "--headless",
            "--ticks",
            "5",
            "--safe-settings",
        ],
        &[
            "--cs-path",
            "/install",
            "--mission",
            "M01",
            "--safe-settings",
        ],
        &["--cs-path", "/install", "--safe-settings"],
        &["--joystick-probe", "--safe-settings"],
    ] {
        match cli::parse(args(refused)) {
            CliRequest::Invalid { reason } => {
                assert!(reason.contains("--safe-settings"), "{reason}");
            }
            other => panic!("{refused:?} must be invalid input, got {other:?}"),
        }
    }
}

#[test]
fn accept_f52_w1_help_documents_the_flag() {
    assert!(cli::HELP_TEXT.contains("--safe-settings"));
    assert!(cli::HELP_TEXT.contains("accessibility.settings"));
}

#[test]
fn accept_f52_w1_the_session_opens_the_per_user_file_under_the_base() {
    let base = scratch_dir("accept_f52_w1_path");
    let session = open(false, &base);
    assert_eq!(session.path(), base.join("accessibility.settings"));
    assert_eq!(
        session.path(),
        user_data::accessibility_settings_path(&base)
    );
    assert!(
        cli::settings_origin_message(&session).contains("no accessibility settings file"),
        "{}",
        cli::settings_origin_message(&session)
    );
    // Opening never writes.
    assert!(!session.path().exists());

    // Without a profile base the player's base is chosen from the environment;
    // an environment that names none is an error, never a guess.
    let error =
        cli::open_accessibility_session(false, None, Platform::Linux, &Environment::default())
            .expect_err("no HOME and no profile base");
    assert!(error.to_string().contains("HOME"), "{error}");
    let env = Environment {
        home: Some(PathBuf::from("/home/p")),
        ..Environment::default()
    };
    let session = cli::open_accessibility_session(false, None, Platform::Linux, &env).unwrap();
    assert_eq!(
        session.path(),
        PathBuf::from("/home/p/.local/share/crimson-skies/accessibility.settings")
    );
}

#[test]
fn accept_f52_w1_a_saved_file_loads_and_safe_settings_skips_it() {
    let base = scratch_dir("accept_f52_w1_safe");
    let mut first = open(false, &base);
    first
        .apply(|settings| settings.presentation.ui_scale_percent = 150)
        .expect("the change persists");
    first.teardown().expect("teardown flushes");

    let loaded = open(false, &base);
    assert_eq!(loaded.settings().presentation.ui_scale_percent, 150);
    assert!(cli::settings_origin_message(&loaded).contains("loaded from"));

    let safe = open(true, &base);
    assert_eq!(safe.settings().presentation.ui_scale_percent, 100);
    let message = cli::settings_origin_message(&safe);
    assert!(message.contains("--safe-settings"), "{message}");
    assert!(message.contains("was not read"), "{message}");
    // The saved file is left as it was.
    assert_eq!(
        open(false, &base).settings().presentation.ui_scale_percent,
        150
    );
}

#[test]
fn accept_f52_w1_a_recovered_file_is_reported_with_its_reason_and_left_alone() {
    let base = scratch_dir("accept_f52_w1_recovered");
    let file = base.join("accessibility.settings");
    fs::write(&file, "ui_scale_percent = not a number\n\0garbage").unwrap();
    let before = fs::read(&file).unwrap();

    let session = open(false, &base);
    let reason = session
        .recovery()
        .expect("an unusable file is a recovery, not a silent default");
    let message = cli::settings_origin_message(&session);
    assert!(message.contains("unusable"), "{message}");
    assert!(message.contains(&reason.to_string()), "{message}");
    assert!(message.contains(&file.display().to_string()), "{message}");
    assert_eq!(session.settings().presentation.ui_scale_percent, 100);
    assert_eq!(fs::read(&file).unwrap(), before, "the file is untouched");

    // --safe-settings does not read it, so there is nothing to recover from.
    let safe = open(true, &base);
    assert!(safe.recovery().is_none());
}
