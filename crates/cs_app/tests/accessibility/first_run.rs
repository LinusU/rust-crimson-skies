//! First run of the settings session: the user-data directory does not exist
//! yet (SMOKE-SETTINGS-EXIT-1). Persisting creates it; a real write failure
//! is still reported.

use std::fs;
use std::path::PathBuf;

use cs_app::accessibility::session::{ApplyError, SettingsSession};

fn base(name: &str) -> PathBuf {
    std::env::temp_dir().join(format!("cs-first-run-{name}-{}", std::process::id()))
}

#[test]
fn accept_settings_first_run_creates_the_missing_user_data_directory() {
    let root = base("missing");
    let _ = fs::remove_dir_all(&root);
    let path = root.join("nested").join("accessibility.settings");
    assert!(!root.exists());

    let mut session = SettingsSession::open(&path, false);
    session
        .apply(|settings| settings.presentation.subtitles = true)
        .expect("the change is live and saved");
    session.teardown().expect("teardown reports success");

    assert!(path.is_file(), "the settings file was written");
    let reopened = SettingsSession::open(&path, false);
    assert!(reopened.settings().presentation.subtitles);
    let _ = fs::remove_dir_all(&root);
}

#[test]
fn accept_settings_first_run_still_reports_a_real_write_failure() {
    let root = base("blocked");
    let _ = fs::remove_dir_all(&root);
    fs::write(&root, b"a file where the directory should be").expect("block");
    let path = root.join("accessibility.settings");

    let mut session = SettingsSession::open(&path, false);
    let result = session.apply(|settings| settings.presentation.subtitles = true);
    assert!(matches!(result, Err(ApplyError::Persist(_))));
    assert!(matches!(session.teardown(), Err(ApplyError::Persist(_))));
    let _ = fs::remove_file(&root);
}
