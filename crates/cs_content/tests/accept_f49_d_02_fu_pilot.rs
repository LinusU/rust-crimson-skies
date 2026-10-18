//! F49-D.02-FU-PILOT: the Instant Action pilot identities, measured from the
//! installation's own `ia.zrd` `ace_name` labels by
//! `IaDenominator::ace_pilot_ids` and carried into the retail option table.
//!
//! The synthetic tests build reader archives in a temporary tree and drive the
//! production survey over it; the retail tests re-measure the installation.

use cs_content::instant_action::{
    OptionListEvidence, PILOT_OPTION_READER, ScenarioOptionDimension, survey_retail_instant_action,
};
use cs_types::content::ContentKind;

use crate::accept_f49_d_01_ia_denominator::{
    TempInstall, add_directory, campaign_tree, zrd_list, zrd_text,
};

/// An `ia.zrd` root that declares only a mission type, spawn points and an ace.
fn ia_with_ace(ace_name: Option<&str>) -> Vec<u8> {
    let mut children = vec![
        zrd_text("mission_type"),
        zrd_list(vec![zrd_text("dogfight_ace")]),
        zrd_text("spawn_points"),
        zrd_list(vec![zrd_text("dogfight_ace"), zrd_list(Vec::new())]),
    ];
    if let Some(name) = ace_name {
        children.push(zrd_text("ace_name"));
        children.push(zrd_list(vec![zrd_text(name)]));
    }
    zrd_list(children)
}

fn tree(aces: &[(&str, Option<&str>)]) -> TempInstall {
    let temp = campaign_tree("fu-pilot");
    for (leaf, ace) in aces {
        add_directory(&temp, "C1", leaf, ia_with_ace(*ace));
    }
    temp
}

#[test]
fn accept_f49_d_02_fu_pilot_distinct_ace_labels_become_sorted_pilot_ids() {
    let temp = tree(&[
        ("IA1", Some("MSG_ZED_NAME")),
        ("IA2", Some("MSG_ABLE_NAME")),
        ("IA3", Some("MSG_ZED_NAME")),
        ("IA4", None),
    ]);
    let survey = survey_retail_instant_action(&temp.0).expect("the survey reads");
    let ids: Vec<String> = survey
        .ace_pilot_ids()
        .expect("labels are valid keys")
        .iter()
        .map(ToString::to_string)
        .collect();
    assert_eq!(ids, ["pilot/msg_able_name", "pilot/msg_zed_name"]);
}

#[test]
fn accept_f49_d_02_fu_pilot_no_declared_ace_yields_no_pilot() {
    let temp = tree(&[("IA1", None)]);
    let survey = survey_retail_instant_action(&temp.0).expect("the survey reads");
    assert!(survey.ace_pilot_ids().expect("no label").is_empty());
}

fn game_dir() -> std::path::PathBuf {
    std::path::PathBuf::from(
        std::env::var("CS_GAME_DIR").expect("CS_GAME_DIR must name the original installation"),
    )
}

/// The retail aces: eight directories, seven distinct names (the khan label
/// serves two directories), and the option table offers exactly those ids.
#[test]
#[ignore = "requires CS_GAME_DIR"]
fn accept_f49_d_02_fu_pilot_retail_aces_populate_the_option_table_pilots() {
    let dir = game_dir();
    let survey = survey_retail_instant_action(&dir).expect("the retail survey reads");
    let labels: std::collections::BTreeSet<&str> = survey
        .directories()
        .iter()
        .filter_map(|d| d.aircraft().ace().name_label())
        .collect();
    assert_eq!(survey.directories().len(), 8);
    assert_eq!(
        labels.len(),
        7,
        "seven distinct ace name labels: {labels:?}"
    );
    let pilots = survey.ace_pilot_ids().expect("labels are valid keys");
    assert_eq!(pilots.len(), 7);
    assert!(pilots.iter().all(|id| id.kind() == ContentKind::Pilot));

    let table = cs_content::instant_action::retail_scenario_options(&dir, Vec::new())
        .expect("the retail option table assembles");
    assert_eq!(table.pilots(), pilots.as_slice());
    match table
        .evidence(ScenarioOptionDimension::Pilots)
        .expect("a named dimension")
    {
        OptionListEvidence::Measured { reader } => assert_eq!(*reader, PILOT_OPTION_READER),
        other => panic!("pilots must be measured, got {other:?}"),
    }
}
