//! Task F48-C-FOLLOWUP-SETTINGS-CATALOG: the engine's settings keys are
//! declared and the runtime session that a run actually opens validates them.
//! The keys, ranges and defaults are engine-designed, not original-game
//! settings (`docs/findings/2026-10-25-f48-c-followup-settings-catalog-keys.md`).

use std::fs;
use std::path::PathBuf;

use cs_app::ui::front_end::pilot_progress::{chain_profile_base, open_session};
use cs_content::settings::{ENGINE_SETTING_RULES, Settings, engine_catalog};
use cs_types::profile::{SettingApply, SettingEntry};

struct Dir(PathBuf);

impl Dir {
    fn new(name: &str) -> Self {
        let root = std::env::temp_dir().join(format!(
            "cs-f48-c-fu-{name}-{}-{:?}",
            std::process::id(),
            std::thread::current().id()
        ));
        let _ = fs::remove_dir_all(&root);
        fs::create_dir_all(&root).expect("dir");
        Self(root)
    }
}

impl Drop for Dir {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

#[test]
fn accept_f48_c_followup_the_engine_catalog_holds_together_and_matches_the_designed_settings() {
    let _ = engine_catalog();
    let designed = Settings::designed();
    let text = designed.to_text();
    assert_eq!(ENGINE_SETTING_RULES.len(), 10);
    for rule in &ENGINE_SETTING_RULES {
        assert!(rule.is_valid(), "{} is recoverable", rule.key);
    }
    // Every declared default is the designed value of the setting it names.
    let designed_value = |file_key: &str| {
        text.lines()
            .find_map(|line| line.strip_prefix(&format!("{file_key}=")))
            .unwrap_or_else(|| panic!("{file_key} is a designed setting"))
            .to_owned()
    };
    for rule in &ENGINE_SETTING_RULES {
        let file_key = rule.key.split_once('.').expect("a dotted key").1;
        assert_eq!(rule.default, designed_value(file_key), "{}", rule.key);
    }
    // Display and rule-set changes are restart-required; the rest is live.
    let apply = |key: &str| {
        ENGINE_SETTING_RULES
            .iter()
            .find(|rule| rule.key == key)
            .map(|rule| rule.apply)
    };
    assert_eq!(apply("display.width"), Some(SettingApply::RestartRequired));
    assert_eq!(apply("modern.profile"), Some(SettingApply::RestartRequired));
    assert_eq!(apply("accessibility.subtitles"), Some(SettingApply::Live));
}

#[test]
fn accept_f48_c_followup_an_unusable_value_recovers_to_the_default_through_the_runtime_session() {
    let capture = Dir::new("recover");
    let base = chain_profile_base(&capture.0);
    {
        let mut session = open_session(Some(&capture.0)).expect("open the runtime session");
        session.create("Pilot").expect("create");
        let mut document = session.document().expect("a document").clone();
        document.settings = vec![
            SettingEntry {
                key: "accessibility.ui_scale_percent".to_owned(),
                apply: SettingApply::Live,
                value: "9000".to_owned(),
            },
            SettingEntry {
                key: "accessibility.colour_filter".to_owned(),
                apply: SettingApply::Live,
                value: "protanopia".to_owned(),
            },
        ];
        document.revision = document.revision.next().expect("a successor");
        session.library_mut().save(&document).expect("write");
        session.finish().expect("teardown");
    }
    let session = open_session(Some(&capture.0)).expect("reopen");
    let settings = session.settings().expect("settings");
    assert_eq!(
        settings.live_value("accessibility.ui_scale_percent"),
        Some("100"),
        "a scale outside the declared range recovers to the designed default"
    );
    assert_eq!(
        settings.live_value("accessibility.colour_filter"),
        Some("protanopia"),
        "a usable value is kept"
    );
    assert!(
        session
            .warnings()
            .iter()
            .any(|line| line.contains("accessibility.ui_scale_percent=9000")),
        "the recovery is reported: {:?}",
        session.warnings()
    );
    session.finish().expect("teardown");
    assert!(base.exists());
}
