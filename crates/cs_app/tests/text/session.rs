//! F51-C acceptance tests: the localization runtime menus, HUD and subtitles
//! share, the locale teardown/retry, original-font loading and the proof that a
//! locale change round-trips through a save without losing unlocks.
//!
//! Spec: `specs/F51-localization-fonts-text-layout-and-original-media-ids.md`,
//! stage `### F51-C`; shared contract `docs/contracts/UI-NETWORK.md`. Task test
//! prefix: `accept_f51_c_`.
//!
//! Every value here is authored development content in a temporary directory:
//! no test opens `$CS_GAME_DIR` or any original data, and the font measurer is a
//! declared stand-in for the still-unmeasured original format. The locale list,
//! setting key and font metrics are engine vocabulary, not claims about the
//! original release.

use std::fs;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};

use cs_app::profile::ProfileSession;
use cs_app::text::{
    FontLoadError, FontMeasurer, FontSet, HudTextRequest, MenuTextRequest, RequiredControl,
    SubtitleRequest, TextMetrics, TextSession, TextSessionError, synthetic_monospace,
};
use cs_content::localization::{
    FontCatalog, FontFace, LOCALE_SETTING_KEY, LocaleSetting, LocaleSettingError, ResourceDecode,
    SubstitutionTable, TextCatalog, TextId,
};
use cs_content::save::settings::SettingCatalog;
use cs_types::content::{ContentId, ContentKind, Origin, Provenance};
use cs_types::profile::{
    ProfileDocument, ProfileId, RecordEntry, Revision, SettingApply, SettingEntry,
};

use crate::common::{
    PANEL, buttons, chain, claim, control_id, grammar, language_map, locale, resource_row,
    substitutions,
};

/// The supported locale labels this test declares, as the caller of the
/// localization setting must. Not a claim about the original release.
const SUPPORTED: &[&str] = &["en-us", "de-de", "fr-fr"];
const NO_LABELS: &[&str] = &[];

/// The three synthetic families: one menu row, one HUD readout and one cue, each
/// in English (1033) and German (1031). The German rows are deliberately ASCII
/// so a text assertion is not confounded by a missing-glyph diagnostic.
fn decoded_catalog() -> TextCatalog {
    let rows = [
        resource_row(10, 1033, Some("New Game")),
        resource_row(10, 1031, Some("Neues Spiel")),
        resource_row(11, 1033, Some("Airspeed")),
        resource_row(11, 1031, Some("Fahrt")),
        resource_row(12, 1033, Some("Bandit on your six")),
        resource_row(12, 1031, Some("Bandit hinter dir")),
    ];
    ResourceDecode::decode(
        &rows,
        &language_map(&[(1033, "en-us"), (1031, "de-de")]),
        Origin::SyntheticFixture,
        Provenance::designed(claim()),
    )
    .into_catalog()
}

/// A catalog with English rows only, for a locale switch that cannot answer.
fn english_only_catalog() -> TextCatalog {
    let rows = [resource_row(10, 1033, Some("New Game"))];
    ResourceDecode::decode(
        &rows,
        &language_map(&[(1033, "en-us")]),
        Origin::SyntheticFixture,
        Provenance::designed(claim()),
    )
    .into_catalog()
}

/// The declared font the fixtures use: a licensed fallback with verified
/// permission and ASCII coverage (the F51-A synthetic face).
fn font_id() -> ContentId {
    cs_content::localization::synthetic_font_face().id().clone()
}

/// A measurer that stands in for the still-unmeasured original font format: it
/// returns the declared monospace metrics, which is exactly the seam F51-D's
/// real decode will fill.
struct SyntheticMeasurer;

impl FontMeasurer for SyntheticMeasurer {
    fn measure(&self, _face: &FontFace) -> Result<TextMetrics, String> {
        Ok(synthetic_monospace(16.0))
    }
}

/// A measurer that cannot read the original format, so the load must fail
/// loudly rather than lay text out from invented numbers.
struct UnsupportedMeasurer;

impl FontMeasurer for UnsupportedMeasurer {
    fn measure(&self, _face: &FontFace) -> Result<TextMetrics, String> {
        Err("the original font format is unmeasured".to_owned())
    }
}

/// The declared face as a one-entry catalog.
fn font_catalog() -> FontCatalog {
    let mut catalog = FontCatalog::new();
    catalog
        .insert(cs_content::localization::synthetic_font_face())
        .expect("the fixture face is inserted once");
    catalog
}

/// A loaded font set built the production way: every declared face measured.
fn loaded_fonts() -> FontSet {
    FontSet::load(&font_catalog(), &SyntheticMeasurer).expect("the synthetic face is measurable")
}

/// A session in `en-us` over `catalog`, with the fixture font loaded.
fn english_session(catalog: TextCatalog) -> TextSession {
    TextSession::try_new(
        chain("en-us", &[]),
        catalog,
        grammar(),
        loaded_fonts(),
        font_id(),
    )
    .expect("the session's default font is loaded")
}

/// A menu request with the session's default font, borrowing its parts for as
/// long as the request lives. A closure cannot express this relation, so the
/// tests share one helper.
fn menu_request<'a>(
    element: &'a ContentId,
    label: &'a TextId,
    required: &'a [RequiredControl],
    substitutions: &'a SubstitutionTable,
) -> MenuTextRequest<'a> {
    MenuTextRequest {
        element,
        label,
        panel: PANEL,
        required,
        substitutions,
        font: None,
    }
}

/// A chain whose selected locale is `selected`, falling back to English when it
/// is not already English.
fn chain_from(
    selected: cs_content::localization::LocaleId,
) -> cs_content::localization::LocaleChain {
    let fallback = if selected.as_str() == "en-us" {
        Vec::new()
    } else {
        vec![locale("en-us")]
    };
    chain(
        selected.as_str(),
        &fallback.iter().map(|l| l.as_str()).collect::<Vec<_>>(),
    )
}

/// A disposable user-data base, removed on drop. Mirrors the F48-C tests: no
/// test touches a real profile tree.
struct TempBase(PathBuf);

impl TempBase {
    fn new(label: &str) -> Self {
        static NEXT: AtomicU64 = AtomicU64::new(0);
        let root = std::env::temp_dir().join(format!(
            "cs-f51-c-{label}-{}-{}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ));
        let _ = fs::remove_dir_all(&root);
        fs::create_dir_all(&root).expect("the fixture base is created");
        Self(root)
    }

    fn path(&self) -> &Path {
        &self.0
    }
}

impl Drop for TempBase {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

/// A settings catalog declaring the localization feature's own rule.
fn setting_catalog() -> SettingCatalog {
    SettingCatalog::new([LocaleSetting::rule(SUPPORTED).expect("the locale rule is valid")])
        .expect("the fixture settings catalog holds together")
}

/// The minimum scenario (AC03): switch locale and reopen the same save without
/// losing unlocks. The locale lives in the profile's *settings* list, so the
/// campaign, records and blueprints are byte-for-byte what they were, and the
/// runtime rebuilt from the reopened save resolves the new locale's text.
#[test]
fn accept_f51_c_switching_locale_and_reopening_the_save_keeps_unlocks() {
    let base = TempBase::new("locale-save");
    let catalog = decoded_catalog();
    let settings = setting_catalog();
    let blueprint =
        ContentId::from_source(ContentKind::Blueprint, "sparrow").expect("a valid blueprint id");
    let record = RecordEntry {
        key: "mission.m01.cleared".to_owned(),
        value: 1,
    };

    // Create a profile, record unlocks and select English.
    let id = {
        let mut session = ProfileSession::open_sandbox(base.path(), &settings)
            .expect("open the synthetic population");
        let id = session.create("Pilot").expect("create the profile");
        session
            .commit_with(|document| {
                document.blueprints.push(blueprint.clone());
                document.records.push(record.clone());
                document.campaign.money_minor = 500;
                Ok(())
            })
            .expect("the unlocks commit");
        session
            .set_setting(LOCALE_SETTING_KEY, "en-us")
            .expect("the locale setting is declared");
        session.commit().expect("the locale commits");
        id
    };

    // The runtime in the stored locale.
    let element = control_id("menu.new_game");
    let label = TextId::from_resource_id(10);
    let required = buttons();
    let subs = substitutions();
    let mut text = english_session(catalog.clone());
    let menu = text
        .menu_text(&MenuTextRequest {
            element: &element,
            label: &label,
            panel: PANEL,
            required: &required,
            substitutions: &subs,
            font: None,
        })
        .expect("the English label resolves");
    assert_eq!(menu.text().layout().text(), "New Game");
    assert_eq!(menu.element(), &element);

    // Switch the stored locale and the running locale.
    {
        let mut session = ProfileSession::open_sandbox(base.path(), &settings)
            .expect("reopen to switch the locale");
        session.select(id).expect("select the profile");
        session
            .set_setting(LOCALE_SETTING_KEY, "de-de")
            .expect("the German label is declared");
        session.commit().expect("the locale commits");
        session.finish().expect("the session ends cleanly");
    }
    text.switch_locale(chain("de-de", &["en-us"]), catalog.clone())
        .expect("the German locale answers");
    let menu_de = text
        .menu_text(&MenuTextRequest {
            element: &element,
            label: &label,
            panel: PANEL,
            required: &required,
            substitutions: &subs,
            font: None,
        })
        .expect("the German label resolves");
    assert_eq!(menu_de.text().layout().text(), "Neues Spiel");
    assert_eq!(
        menu_de.element(),
        &element,
        "the element identity is locale-free"
    );

    // Reopen the same save: unlocks are untouched, the locale survives.
    let mut reopened =
        ProfileSession::open_sandbox(base.path(), &settings).expect("reopen the population");
    reopened.select(id).expect("select the saved profile");
    let document = reopened.document().expect("the save has a document");
    assert_eq!(document.blueprints, vec![blueprint]);
    assert_eq!(document.records, vec![record]);
    assert_eq!(document.campaign.money_minor, 500);
    assert_eq!(document.profile_id, id);
    let stored = LocaleSetting::read(document)
        .expect("the stored locale is readable")
        .expect("the save carries a locale");
    assert_eq!(stored.as_str(), "de-de");

    // The runtime rebuilt from the reopened save's own locale resolves German.
    let rebuilt = TextSession::try_new(
        chain_from(stored),
        catalog,
        grammar(),
        loaded_fonts(),
        font_id(),
    )
    .expect("the rebuilt session's default font is loaded");
    let rebuilt_menu = rebuilt
        .menu_text(&MenuTextRequest {
            element: &element,
            label: &label,
            panel: PANEL,
            required: &required,
            substitutions: &subs,
            font: None,
        })
        .expect("the reopened save's locale answers");
    assert_eq!(rebuilt_menu.text().layout().text(), "Neues Spiel");
    assert_eq!(rebuilt_menu.element(), &element);
}

/// A locale switch that cannot answer its chain is refused, and the previous
/// locale stays fully in force: a half-loaded locale never blanks the screen,
/// and the caller retries after the content is repaired.
#[test]
fn accept_f51_c_a_refused_locale_switch_keeps_the_previous_catalog() {
    let mut session = english_session(decoded_catalog());
    let element = control_id("menu.new_game");
    let label = TextId::from_resource_id(10);
    let required = buttons();
    let subs = substitutions();

    let before = session
        .menu_text(&menu_request(&element, &label, &required, &subs))
        .expect("the English label resolves");
    assert_eq!(before.text().layout().text(), "New Game");

    let error = session
        .switch_locale(chain("fr-fr", &[]), english_only_catalog())
        .expect_err("a French chain over an English catalog answers nothing");
    match error {
        TextSessionError::NoContentForLocale { chain } => {
            assert_eq!(chain, vec![locale("fr-fr")]);
        }
        other => panic!("expected a refused locale switch, got {other:?}"),
    }

    // The previous locale is still installed and still resolves.
    let after = session
        .menu_text(&menu_request(&element, &label, &required, &subs))
        .expect("the previous locale is untouched");
    assert_eq!(after.text().layout().text(), "New Game");
    assert_eq!(after.text().locale().as_str(), "en-us");

    // The retry succeeds once a catalog that answers the chain is offered.
    session
        .switch_locale(chain("de-de", &["en-us"]), decoded_catalog())
        .expect("the retry answers the German chain");
    let german = session
        .menu_text(&menu_request(&element, &label, &required, &subs))
        .expect("the German label resolves");
    assert_eq!(german.text().layout().text(), "Neues Spiel");
    assert_eq!(german.text().locale().as_str(), "de-de");
}

/// Menus, HUD and subtitles all resolve through one session and bind their text
/// to locale-free identity: the menu element, the HUD field and the subtitle's
/// speaker, cue id and tick window do not change when the locale does.
#[test]
fn accept_f51_c_menus_hud_and_subtitles_bind_locale_independent_identity() {
    let catalog = decoded_catalog();
    let mut session = english_session(catalog.clone());
    let element = control_id("menu.new_game");
    let field = control_id("hud.airspeed");
    let menu_label = TextId::from_resource_id(10);
    let hud_label = TextId::from_resource_id(11);
    let cue = TextId::from_resource_id(12);
    let required = buttons();
    let subs = substitutions();

    let menu = session
        .menu_text(&MenuTextRequest {
            element: &element,
            label: &menu_label,
            panel: PANEL,
            required: &required,
            substitutions: &subs,
            font: None,
        })
        .expect("the menu label resolves");
    let hud = session
        .hud_text(&HudTextRequest {
            field: &field,
            label: &hud_label,
            panel: PANEL,
            required: &required,
            substitutions: &subs,
            font: None,
        })
        .expect("the HUD label resolves");
    let subtitle = session
        .subtitle_text(&SubtitleRequest {
            speaker: "Control",
            cue: &cue,
            start_tick: 100,
            duration_ticks: 30,
            panel: PANEL,
            required: &required,
            substitutions: &subs,
            font: None,
        })
        .expect("the cue resolves");

    assert_eq!(menu.text().layout().text(), "New Game");
    assert_eq!(hud.text().layout().text(), "Airspeed");
    assert_eq!(subtitle.text().layout().text(), "Bandit on your six");
    assert_eq!(menu.element(), &element);
    assert_eq!(hud.field(), &field);
    assert_eq!(subtitle.speaker(), "Control");
    assert_eq!(subtitle.cue(), &cue);
    assert_eq!(subtitle.start_tick(), 100);
    assert_eq!(subtitle.end_tick(), 130);
    assert!(!subtitle.is_visible_at(99));
    assert!(subtitle.is_visible_at(100));
    assert!(subtitle.is_visible_at(129));
    assert!(!subtitle.is_visible_at(130));

    session
        .switch_locale(chain("de-de", &["en-us"]), catalog)
        .expect("the German locale answers");

    let menu_de = session
        .menu_text(&MenuTextRequest {
            element: &element,
            label: &menu_label,
            panel: PANEL,
            required: &required,
            substitutions: &subs,
            font: None,
        })
        .expect("the German menu label resolves");
    let hud_de = session
        .hud_text(&HudTextRequest {
            field: &field,
            label: &hud_label,
            panel: PANEL,
            required: &required,
            substitutions: &subs,
            font: None,
        })
        .expect("the German HUD label resolves");
    let subtitle_de = session
        .subtitle_text(&SubtitleRequest {
            speaker: "Control",
            cue: &cue,
            start_tick: 100,
            duration_ticks: 30,
            panel: PANEL,
            required: &required,
            substitutions: &subs,
            font: None,
        })
        .expect("the German cue resolves");

    assert_eq!(menu_de.text().layout().text(), "Neues Spiel");
    assert_eq!(hud_de.text().layout().text(), "Fahrt");
    assert_eq!(subtitle_de.text().layout().text(), "Bandit hinter dir");
    assert_eq!(
        menu_de.element(),
        &element,
        "locale does not change identity"
    );
    assert_eq!(hud_de.field(), &field);
    assert_eq!(subtitle_de.speaker(), "Control");
    assert_eq!(subtitle_de.cue(), &cue);
    assert_eq!(subtitle_de.start_tick(), 100);
    assert_eq!(subtitle_de.end_tick(), 130);
}

/// A subtitle with no speaker, no duration or an overflowing tick window is
/// refused before it is bound, rather than shown as an invisible line.
#[test]
fn accept_f51_c_a_subtitle_requires_a_speaker_and_a_nonzero_window() {
    let session = english_session(decoded_catalog());
    let cue = TextId::from_resource_id(12);
    let required = buttons();
    let subs = substitutions();
    let request = |speaker: &'static str, start: u64, duration: u64| SubtitleRequest {
        speaker,
        cue: &cue,
        start_tick: start,
        duration_ticks: duration,
        panel: PANEL,
        required: &required,
        substitutions: &subs,
        font: None,
    };

    assert_eq!(
        session.subtitle_text(&request("   ", 0, 10)),
        Err(TextSessionError::EmptySpeaker)
    );
    assert_eq!(
        session.subtitle_text(&request("Control", 0, 0)),
        Err(TextSessionError::ZeroDuration)
    );
    assert_eq!(
        session.subtitle_text(&request("Control", u64::MAX, 1)),
        Err(TextSessionError::TickOverflow {
            start_tick: u64::MAX,
            duration_ticks: 1,
        })
    );
    // A cue no locale answers stays a named miss, never a blank subtitle.
    let missing = TextId::from_resource_id(99);
    assert!(matches!(
        session.subtitle_text(&SubtitleRequest {
            speaker: "Control",
            cue: &missing,
            start_tick: 0,
            duration_ticks: 10,
            panel: PANEL,
            required: &required,
            substitutions: &subs,
            font: None,
        }),
        Err(TextSessionError::Missing { .. })
    ));
}

/// Font loading is a whole-set transaction: one unmeasurable face fails the
/// load, a failed face is not stored, a retry succeeds and a session cannot be
/// built without its default font.
#[test]
fn accept_f51_c_font_loading_propagates_failure_and_retries() {
    let face = cs_content::localization::synthetic_font_face();
    let catalog = font_catalog();

    assert_eq!(
        FontSet::load(&catalog, &UnsupportedMeasurer).unwrap_err(),
        FontLoadError::Unmeasured {
            family: face.family().to_owned(),
            reason: "the original font format is unmeasured".to_owned(),
        }
    );
    assert_eq!(
        FontSet::load(&FontCatalog::new(), &SyntheticMeasurer).unwrap_err(),
        FontLoadError::Empty
    );

    let mut fonts = FontSet::load(&catalog, &SyntheticMeasurer).expect("the face is measurable");
    assert!(fonts.contains(face.id()));
    assert!(matches!(
        fonts
            .insert(face.clone(), synthetic_monospace(16.0))
            .unwrap_err(),
        FontLoadError::Duplicate { .. }
    ));

    // A failed reload leaves whatever was loaded in place.
    let mut session = english_session(decoded_catalog());
    let element = control_id("menu.new_game");
    let label = TextId::from_resource_id(10);
    let required = buttons();
    let subs = substitutions();
    assert!(session.retry_font(&face, &UnsupportedMeasurer).is_err());
    assert_eq!(
        session
            .menu_text(&MenuTextRequest {
                element: &element,
                label: &label,
                panel: PANEL,
                required: &required,
                substitutions: &subs,
                font: None,
            })
            .expect("the previously loaded font is untouched")
            .text()
            .layout()
            .text(),
        "New Game"
    );
    session
        .retry_font(&face, &SyntheticMeasurer)
        .expect("the retry succeeds");

    // A session with no font at all is refused, so text is never laid out with
    // unmeasured metrics.
    let error = TextSession::try_new(
        chain("en-us", &[]),
        decoded_catalog(),
        grammar(),
        FontSet::new(),
        face.id().clone(),
    )
    .expect_err("a session needs its default font loaded");
    assert_eq!(
        error,
        TextSessionError::MissingFont {
            id: face.id().clone()
        }
    );
}

/// The locale setting bridge is a settings-only edit: it never disturbs
/// campaign, records, blueprints or save identity, it replaces in place rather
/// than duplicating, it refuses a damaged duplicate key, and its rule owns the
/// key, the live apply and the declared value space.
#[test]
fn accept_f51_c_the_locale_setting_preserves_unlocks_and_save_identity() {
    let mut document = ProfileDocument::synthetic(
        ProfileId::new(1).expect("a nonzero profile id"),
        Revision(1),
    );
    let blueprint =
        ContentId::from_source(ContentKind::Blueprint, "sparrow").expect("a valid blueprint id");
    document.blueprints.push(blueprint);
    document.records.push(RecordEntry {
        key: "mission.m01.cleared".to_owned(),
        value: 3,
    });
    document.campaign.money_minor = 42;
    let before = document.clone();

    LocaleSetting::write(&mut document, &locale("de-de")).expect("the locale is written");
    assert_eq!(
        LocaleSetting::read(&document)
            .expect("readable")
            .expect("present")
            .as_str(),
        "de-de"
    );
    LocaleSetting::write(&mut document, &locale("fr-fr")).expect("the locale is rewritten");
    assert_eq!(
        document
            .settings
            .iter()
            .filter(|entry| entry.key == LOCALE_SETTING_KEY)
            .count(),
        1,
        "a rewrite replaces in place rather than appending a duplicate"
    );
    assert_eq!(
        LocaleSetting::read(&document)
            .expect("readable")
            .expect("present")
            .as_str(),
        "fr-fr"
    );

    assert_eq!(document.blueprints, before.blueprints);
    assert_eq!(document.records, before.records);
    assert_eq!(document.campaign, before.campaign);
    assert_eq!(document.profile_id, before.profile_id);
    assert_eq!(document.display_name, before.display_name);

    // A damaged save that stores the key twice is refused, not silently rewritten.
    let mut duplicate = before.clone();
    for value in ["en-us", "de-de"] {
        duplicate.settings.push(SettingEntry {
            key: LOCALE_SETTING_KEY.to_owned(),
            apply: SettingApply::Live,
            value: value.to_owned(),
        });
    }
    assert_eq!(
        LocaleSetting::write(&mut duplicate, &locale("fr-fr")),
        Err(LocaleSettingError::DuplicateEntry)
    );
    assert_eq!(
        LocaleSetting::read(&duplicate),
        Err(LocaleSettingError::DuplicateEntry)
    );

    // The rule the localization feature owns.
    assert_eq!(
        LocaleSetting::rule(NO_LABELS),
        Err(LocaleSettingError::NoLabels)
    );
    assert_eq!(
        LocaleSetting::rule(&["en-us", "en-us"]),
        Err(LocaleSettingError::DuplicateLabel {
            label: "en-us".to_owned()
        })
    );
    let rule = LocaleSetting::rule(SUPPORTED).expect("the supported set is valid");
    assert_eq!(rule.key, LOCALE_SETTING_KEY);
    assert_eq!(rule.apply, SettingApply::Live);
    assert_eq!(rule.default, "en-us");
}
