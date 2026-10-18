//! Acceptance scenario F49-D.02, retail half: the environment dimension of the
//! custom-scenario option table, and the airframe display-name mapping the
//! table's airframe rows answer to.
//!
//! Spec: `specs/F49-instant-action-presets-and-custom-scenarios.md` (deliverable
//! "all supported custom-scenario dimensions discovered from data/UI"). Task
//! test prefix: `accept_f49_d_02_`. Required capability: **retail**
//! (`$CS_GAME_DIR`).
//!
//! This test lives in `cs_app` because it needs two producers that sit above
//! `cs_content`:
//!
//! * `cs_app::environment::retail::read_mission_weather`, the production
//!   weather reader, supplies the measured environment ids the option table's
//!   `environments` list carries (F19/F20);
//! * `cs_app::mission_start::airframe_entry`, the measured display-name ->
//!   airframe-row mapping, resolves each `player_plane` spelling onto the same
//!   `airframe/<scene_root>` row the F14-D.6 collection declares — so the
//!   option table's airframe list and the mapping cannot disagree.
//!
//! It reads the owner's installation read-only and commits nothing; run it with
//! `--include-ignored`. Without `CS_GAME_DIR` it fails loudly.

use std::path::PathBuf;

use cs_content::instant_action::{
    OptionListEvidence, ScenarioOptionDimension, ScenarioSchemaError, retail_scenario_options,
};
use cs_types::content::{ContentId, ContentKind};

fn game_dir() -> PathBuf {
    PathBuf::from(
        std::env::var("CS_GAME_DIR")
            .expect("CS_GAME_DIR must name the original installation for this retail test"),
    )
}

/// The option table carries the environments the production weather reader
/// measured, and the airframe rows it offers are exactly the ones the measured
/// display-name mapping names.
#[test]
#[ignore = "requires CS_GAME_DIR"]
fn accept_f49_d_02_retail_the_measured_environments_and_airframe_rows_are_offered() {
    let dir = game_dir();
    let discovery =
        cs_assets::install::discover(&dir).expect("production discovery reads the installation");
    let layout = cs_content::campaign_bindings::campaign_layout(&dir)
        .expect("the shared campaign walk reads the layout");

    // The measured environment ids, read by the production weather reader from
    // every campaign mission's own `weather.zrd`.
    let mut environments = Vec::new();
    let mut missions_without_weather = 0usize;
    for entry in &layout {
        let program = entry.mission.program_asset.to_ascii_lowercase();
        let Some(mission_key) = program.strip_suffix("/zrdr.zbd") else {
            continue;
        };
        match cs_app::environment::retail::read_mission_weather(&discovery, mission_key) {
            Ok(weather) => environments.push(weather.binding.definition.id().clone()),
            Err(_) => missions_without_weather += 1,
        }
    }
    assert!(
        !environments.is_empty(),
        "the installation's missions bind at least one environment through the production \
         weather reader; {missions_without_weather} mission(s) have no weather member"
    );

    let table = retail_scenario_options(&dir, environments.clone())
        .expect("the retail option table assembles");
    assert_eq!(
        table.environments(),
        environments.as_slice(),
        "the option table carries exactly the environments the weather reader measured"
    );
    assert!(
        table
            .evidence(ScenarioOptionDimension::Environments)
            .expect("a named dimension")
            .is_measured(),
        "a caller-supplied, really-measured environment list is tagged measured"
    );

    // The loadout dimension still has no producer, so it stays empty and
    // refuses the table by name rather than being filled in. The pilots are the
    // ia.zrd aces (F49-D.02-FU-PILOT).
    assert_eq!(table.empty_required_lists(), vec!["loadouts"]);
    assert!(!table.pilots().is_empty());
    assert_eq!(
        table
            .clone()
            .validate()
            .expect_err("an unmeasured required list refuses the table"),
        ScenarioSchemaError::EmptyOptionTable { what: "loadouts" }
    );

    // The display-name -> airframe-row mapping: every measured airframe name
    // resolves onto an airframe row the option table offers, by its scene root.
    let mut mapped = Vec::new();
    for entry in cs_app::mission_start::AIRFRAME_TABLE.iter() {
        let (index, row) = cs_app::mission_start::airframe_entry(entry.display_name)
            .unwrap_or_else(|| panic!("{} is a measured airframe name", entry.display_name));
        assert_eq!(
            row.scene_root, entry.scene_root,
            "{}: the mapping and the pinned table agree",
            entry.display_name
        );
        assert_eq!(
            cs_app::mission_start::airframe_index(entry.display_name),
            Some(index),
            "{}: the index and the entry agree",
            entry.display_name
        );
        let id = ContentId::from_source(ContentKind::Airframe, row.scene_root)
            .expect("the scene root is a valid airframe key");
        assert!(
            table.airframes().contains(&id),
            "{} ({}) must be offered by the option table's airframe list",
            entry.display_name,
            id
        );
        mapped.push(id);
    }
    assert!(
        mapped.len() == table.airframes().len(),
        "the {} measured airframe names cover every one of the {} airframe rows",
        mapped.len(),
        table.airframes().len()
    );
    for id in table.airframes() {
        assert!(
            mapped.contains(id),
            "{id} is an airframe row no measured display name resolves to"
        );
    }

    // An unknown name resolves to nothing rather than to a default row.
    assert!(cs_app::mission_start::airframe_entry("no_such_airframe").is_none());

    // The option table's airframe evidence is measured, and no airframe it
    // offers is a fixture id.
    assert!(matches!(
        table
            .evidence(ScenarioOptionDimension::Airframes)
            .expect("a named dimension"),
        OptionListEvidence::Measured { .. }
    ));
    let fixture = cs_content::instant_action::synthetic_scenario_options();
    for id in table.airframes() {
        assert!(
            !fixture.airframes().contains(id),
            "{id} must not be a fixture airframe"
        );
    }
}
