//! F64-D acceptance: the optional old-save migration switch is a real
//! production setting, and turning it off does not touch a new campaign
//! (`specs/F64-legacy-custom-aircraft-and-optional-save-import.md`, stage
//! `### F64-D`, sheet **AC04** — the stage's minimum scenario).
//!
//! F64-C exercised AC04 with a hand-built `ImportContext` boolean. This stage
//! closes the two gaps that left: the switch had no declared setting to read
//! (it is now `cs_content::legacy_import::legacy_save_import_rule`, stored and
//! transactioned by the real profile store), and the consumer consulted it
//! only *after* the layout capability check — which means that in production,
//! where no byte layout has been measured for any class, flipping the switch
//! changed nothing a player could see. Both are pinned here through
//! production code:
//!
//! * `accept_f64_d_optional_save_migration_can_be_disabled_while_new_campaigns_still_work`
//!   — the minimum scenario, end to end through `ProfileSession`: create a
//!   profile, turn the switch off with the store's own setting transaction,
//!   watch an optional save class be refused **by name**, and keep playing the
//!   same profile (campaign run, commit, reopen) with the refusal in place.
//! * `accept_f64_d_the_switch_is_consulted_before_the_missing_layout_capability`
//!   — the ordering: with no layout at all, the switch's refusal wins, and
//!   with the switch on, or for the required class, the missing layout is
//!   still what the screen says.
//! * `accept_f64_d_the_named_switch_is_the_declared_profile_setting` — the
//!   scoping: one constant is both the inventory's `disable_switch` and the
//!   settings key, every optional class carries it, and a value the rule does
//!   not accept recovers to the declared default.
//!
//! Everything synthetic here is authored fixture content offered through the
//! designed layout admission; nothing in this file reads `$CS_GAME_DIR` (the
//! retail test does, in its own file).

#[path = "f64_c_support/mod.rs"]
mod f64_c;
#[path = "f64_d_support/mod.rs"]
mod f64_d;

use cs_app::profile::ProfileSession;
use cs_app::ui::import::{FlowRefusal, ImportFlow, MigrationVerdict};
use cs_content::legacy_import::{
    ImportRefusal, LEGACY_SAVE_IMPORT_OFF, LEGACY_SAVE_IMPORT_ON, LayoutAdmission,
    legacy_save_import_enabled, legacy_save_import_rule,
};
use cs_content::save::settings::{SettingCatalog, SettingOutcome, SettingsState};
use cs_formats::legacy_profile::{
    ImportRequirement, LEGACY_SAVE_IMPORT_SWITCH, LegacyArtifactClass, layout_record,
};
use cs_types::profile::{SettingApply, SettingEntry};
use f64_c::{
    CatalogRows, TempBase, blueprint_document, blueprint_layout_and_map, catalog, full_record,
    id_map, offer, proposal, stock,
};

/// **AC04, the stage's minimum scenario**: optional old-save migration can be
/// disabled while new campaigns still work.
///
/// The switch is read from the profile store, not from a test-owned boolean:
/// `ProfileSession` creates the profile, applies the setting with its own
/// transaction, commits it, and a reopened session reads the same value back.
#[test]
fn accept_f64_d_optional_save_migration_can_be_disabled_while_new_campaigns_still_work() {
    let bytes = blueprint_document(&[full_record()]);
    let save_source = proposal(&bytes, LegacyArtifactClass::CampaignSave)
        .expect("the fixture source is within the cap");
    let plane_source = proposal(&bytes, LegacyArtifactClass::CustomAircraft)
        .expect("the fixture source is within the cap");
    let (layout, map) = blueprint_layout_and_map();
    let ids = id_map();
    let content_catalog = catalog(CatalogRows::Base);
    let (rules, policy, book) = stock();
    let target = f64_c::target();

    // The population declares the switch, so it is a storable setting rather
    // than a string an import happens to check.
    let settings_catalog = f64_d::settings_catalog();
    let base = TempBase::new("f64-d-switch");
    let mut session = ProfileSession::open_sandbox(base.path(), &settings_catalog)
        .expect("the sandbox population opens");

    // A new campaign needs no import: the profile is created with the
    // declared defaults, and the enhancement starts on.
    session
        .create("New campaign")
        .expect("a fresh profile is created with the enhancement on");
    let settings = session.settings().expect("create selects the profile");
    assert!(
        legacy_save_import_enabled(settings),
        "a profile that never stored the switch starts at the declared default"
    );

    // With the switch at its default, an optional save class still reaches a
    // report — so the refusals below are the switch's doing, not the class's.
    let context_on = f64_c::context(&ids, &content_catalog, &rules, &policy, &book, true);
    let mut flow = ImportFlow::new();
    let verdict = flow
        .offer(
            &context_on,
            &offer(&save_source, &bytes, Some(&layout), Some(&map), &target),
        )
        .view()
        .expect("with the switch at its default an optional save still imports")
        .verdict();
    assert_eq!(verdict, MigrationVerdict::Full);

    // The owner's own action: one setting transaction, no import code.
    let applied = session
        .set_setting(LEGACY_SAVE_IMPORT_SWITCH, LEGACY_SAVE_IMPORT_OFF)
        .expect("the switch is a declared setting of this population");
    assert!(
        matches!(applied, SettingOutcome::AppliedLive { .. }),
        "the change takes effect immediately: {applied:?}"
    );
    let enabled =
        legacy_save_import_enabled(session.settings().expect("the profile is still selected"));
    assert!(!enabled, "the store's live value is what the import reads");

    // The same offer is now declined by name, with the switch that did it.
    let context_off = f64_c::context(&ids, &content_catalog, &rules, &policy, &book, enabled);
    let refusal = flow
        .offer(
            &context_off,
            &offer(&save_source, &bytes, Some(&layout), Some(&map), &target),
        )
        .refusal()
        .expect("an optional save class is refused while its switch is off")
        .refusal()
        .clone();
    assert_eq!(refusal.code(), "enhancement_disabled");
    assert!(
        refusal.to_string().contains(LEGACY_SAVE_IMPORT_SWITCH),
        "the refusal names the switch that controls it: {refusal}"
    );
    assert!(
        matches!(
            &refusal,
            FlowRefusal::Plan(ImportRefusal::EnhancementDisabled {
                class: LegacyArtifactClass::CampaignSave,
                switch,
            }) if *switch == LEGACY_SAVE_IMPORT_SWITCH
        ),
        "and it keeps the producer's structured error whole: {refusal:?}"
    );

    // The required class is not behind the switch: the same dialog, with the
    // switch still off, imports a custom aircraft.
    let verdict = flow
        .offer(
            &context_off,
            &offer(&plane_source, &bytes, Some(&layout), Some(&map), &target),
        )
        .view()
        .expect("the required custom-aircraft class is not an optional enhancement")
        .verdict();
    assert_eq!(verdict, MigrationVerdict::Full);

    // New campaigns still work on this very profile while migration is off:
    // a run starts, the profile commits, and nothing is dropped on the way out.
    session
        .begin_campaign_run("f64d.run.1")
        .expect("a campaign run starts with the enhancement off");
    session
        .commit()
        .expect("the profile commits with the enhancement off");
    assert_eq!(
        session
            .campaign()
            .expect("the profile has campaign state")
            .run_id
            .as_deref(),
        Some("f64d.run.1"),
        "the run the store committed is the run the profile carries"
    );
    let report = session.finish().expect("the session tears down cleanly");
    assert!(
        !report.uncommitted_changes,
        "the disabled switch and the run were both written, not dropped: {report:?}"
    );

    // A reopened session reads the stored value back: disabled stays disabled.
    let mut reopened = ProfileSession::open_sandbox(base.path(), &settings_catalog)
        .expect("the population reopens");
    assert!(
        !legacy_save_import_enabled(
            reopened
                .settings()
                .expect("the active profile is selected on open")
        ),
        "the disabled switch is what the save carries"
    );

    // A value the rule does not accept stores nothing and changes nothing —
    // the switch cannot be knocked loose by an unparseable value.
    let outcome = reopened
        .set_setting(LEGACY_SAVE_IMPORT_SWITCH, "maybe")
        .expect("the key is declared, so the value is judged by the rule");
    assert!(
        matches!(outcome, SettingOutcome::Refused { .. }),
        "an unusable value is refused rather than stored: {outcome:?}"
    );
    assert!(
        !legacy_save_import_enabled(reopened.settings().expect("still selected")),
        "the refused value leaves the switch where it was"
    );

    // And the same population still runs a second campaign with it off.
    reopened
        .begin_campaign_run("f64d.run.2")
        .expect("a new campaign starts while the enhancement is off");
    reopened.finish().expect("the session tears down cleanly");
}

/// The switch is decided **before** the layout capability, so a player who
/// turned the enhancement off is told that instead of "no measured layout" —
/// the answer every class gives today, which would hide the switch entirely.
///
/// The flip side is pinned too: with the switch on, and for the required class
/// at any switch position, the missing layout is still the reason.
#[test]
fn accept_f64_d_the_switch_is_consulted_before_the_missing_layout_capability() {
    let bytes = blueprint_document(&[full_record()]);
    let save_source = proposal(&bytes, LegacyArtifactClass::CampaignSave)
        .expect("the fixture source is within the cap");
    let plane_source = proposal(&bytes, LegacyArtifactClass::CustomAircraft)
        .expect("the fixture source is within the cap");
    let ids = id_map();
    let content_catalog = catalog(CatalogRows::Base);
    let (rules, policy, book) = stock();
    let target = f64_c::target();

    // The production state: the measured-only admission and no layout in hand,
    // because `LEGACY_LAYOUT_INVENTORY` is `Unknown` on every row.
    let off = f64_d::context(
        &ids,
        &content_catalog,
        &rules,
        &policy,
        &book,
        false,
        LayoutAdmission::MeasuredOnly,
    );
    let on = f64_d::context(
        &ids,
        &content_catalog,
        &rules,
        &policy,
        &book,
        true,
        LayoutAdmission::MeasuredOnly,
    );
    let mut flow = ImportFlow::new();

    let refusal = flow
        .offer(&off, &offer(&save_source, &bytes, None, None, &target))
        .refusal()
        .expect("an optional save class is declined while its switch is off")
        .refusal()
        .clone();
    assert_eq!(
        refusal.code(),
        "enhancement_disabled",
        "the switch is a class-level decision and is asked first: {refusal:?}"
    );
    assert!(
        refusal.to_string().contains(LEGACY_SAVE_IMPORT_SWITCH),
        "and the refusal still names the switch: {refusal}"
    );

    let code = flow
        .offer(&on, &offer(&save_source, &bytes, None, None, &target))
        .refusal()
        .expect("with the switch on there is still no layout to read")
        .code();
    assert_eq!(
        code, "no_measured_layout",
        "with the enhancement enabled the missing capability is what the screen says"
    );

    let code = flow
        .offer(&off, &offer(&plane_source, &bytes, None, None, &target))
        .refusal()
        .expect("the required class has no measured layout either")
        .code();
    assert_eq!(
        code, "no_measured_layout",
        "the switch never stands between a required class and its own capability"
    );
}

/// One constant is the whole scope of the enhancement: the inventory rows'
/// `disable_switch`, the settings key and the rule's key are the same string,
/// exactly the save classes are optional, and the rule is one a population can
/// actually declare.
#[test]
fn accept_f64_d_the_named_switch_is_the_declared_profile_setting() {
    let rule = legacy_save_import_rule();
    assert_eq!(rule.key, LEGACY_SAVE_IMPORT_SWITCH);
    assert!(rule.is_valid(), "the rule is one it can honor: {rule:?}");
    assert_eq!(
        rule.apply,
        SettingApply::Live,
        "an import dialog reads the switch while it is open"
    );
    assert_eq!(rule.default, LEGACY_SAVE_IMPORT_ON);

    // Every class the inventory calls an optional enhancement is scoped by
    // exactly this switch, and nothing else is optional.
    let mut optional = Vec::new();
    for class in LegacyArtifactClass::ALL {
        let requirement = layout_record(class).requirement;
        if let ImportRequirement::OptionalEnhancement {
            label,
            disable_switch,
        } = requirement
        {
            assert_eq!(
                disable_switch, rule.key,
                "{class} must be disabled by the one declared switch"
            );
            assert_eq!(label, "legacy-save-import", "{class} stays labeled");
            optional.push(class);
        }
    }
    assert_eq!(
        optional,
        [
            LegacyArtifactClass::CampaignSave,
            LegacyArtifactClass::SettingsBlob,
            LegacyArtifactClass::ProfilePointer,
        ],
        "the three save classes are the optional ones"
    );
    assert!(
        layout_record(LegacyArtifactClass::CustomAircraft)
            .requirement
            .is_required(),
        "the construction screen's class stays required and is not switchable"
    );

    // The rule is storable: a population declaring it opens, a stored `off` is
    // `off`, and an unusable value recovers to the declared default.
    let catalog = SettingCatalog::new([rule]).expect("the declared switch is a usable rule");
    let fresh = SettingsState::open(&catalog, &[]);
    assert!(legacy_save_import_enabled(&fresh));
    assert!(
        fresh.refusals().is_empty(),
        "the default needs no recovery: {:?}",
        fresh.refusals()
    );

    let off = SettingsState::open(
        &catalog,
        &[SettingEntry {
            key: LEGACY_SAVE_IMPORT_SWITCH.to_owned(),
            apply: SettingApply::Live,
            value: LEGACY_SAVE_IMPORT_OFF.to_owned(),
        }],
    );
    assert!(!legacy_save_import_enabled(&off));
    assert!(
        off.refusals().is_empty(),
        "a value the rule accepts is not a refusal: {:?}",
        off.refusals()
    );

    let unusable = SettingsState::open(
        &catalog,
        &[SettingEntry {
            key: LEGACY_SAVE_IMPORT_SWITCH.to_owned(),
            apply: SettingApply::Live,
            value: "maybe".to_owned(),
        }],
    );
    assert!(
        legacy_save_import_enabled(&unusable),
        "an unusable stored value recovers to the declared default"
    );
    assert_eq!(
        unusable.refusals().len(),
        1,
        "and says so: {:?}",
        unusable.refusals()
    );

    // A build whose catalog forgot the rule still reads the value this
    // feature's own rule names, and falls back to the default when there is
    // nothing to read — never to a guess.
    let undeclared = SettingCatalog::empty();
    let carried = SettingsState::open(
        &undeclared,
        &[SettingEntry {
            key: LEGACY_SAVE_IMPORT_SWITCH.to_owned(),
            apply: SettingApply::Live,
            value: LEGACY_SAVE_IMPORT_OFF.to_owned(),
        }],
    );
    assert!(!legacy_save_import_enabled(&carried));
    assert!(legacy_save_import_enabled(&SettingsState::open(
        &undeclared,
        &[]
    )));
}
