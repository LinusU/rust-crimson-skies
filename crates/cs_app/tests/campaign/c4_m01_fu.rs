//! C4-M01-FU (#2134): `zbd/c4/m01` objective 24 spells the unquoted authoring
//! note `Change to mobile net`; the original's parser never looks those words
//! up (`docs/findings/2026-10-25-c4-m01-fu-stray-phrase.md`), so the adapter
//! drops them and the row is complete. Retail tests are
//! `#[ignore = "requires CS_GAME_DIR"]`.

use std::path::PathBuf;
use std::sync::OnceLock;

use cs_app::control_lowering::lower_control_record;
use cs_app::mission_control::{RetailControlCensus, survey_mission_control_programs};
use cs_content::mission_control::{CallOutcome, ConditionOutcome, measure_control_record};
use cs_content::stunts::ZrdValue;
use cs_types::content::{ContentId, ContentKind};

const MISSION: &str = "zbd/c4/m01";
const IGNORED: u32 = 4;

fn census() -> &'static RetailControlCensus {
    static CENSUS: OnceLock<RetailControlCensus> = OnceLock::new();
    CENSUS.get_or_init(|| {
        let dir = std::env::var("CS_GAME_DIR").unwrap_or_else(|_| {
            panic!("CS_GAME_DIR is not set: C4-M01-FU needs the retail capability")
        });
        survey_mission_control_programs(&PathBuf::from(dir))
            .expect("the census runs on the installation")
    })
}

fn text(value: &str) -> ZrdValue {
    ZrdValue::Text(value.to_owned())
}

/// Objective 24 as spelled: the phrase's four words sit bare between
/// `BEGIN_DORMANT` and `SET_AI_NET`.
fn phrase_record() -> ZrdValue {
    let pair = |name: &str| ZrdValue::List(vec![text(name), text("M1Intercept")]);
    let block = ZrdValue::List(vec![
        text("BEGIN_DORMANT"),
        ZrdValue::List(vec![ZrdValue::Float(-1.0)]),
        text("Change"),
        text("to"),
        text("mobile"),
        text("net"),
        text("SET_AI_NET"),
        ZrdValue::List(vec![ZrdValue::List(vec![
            pair("bhatbrigand_1"),
            pair("bhatbrigand_2"),
        ])]),
    ]);
    ZrdValue::List(vec![ZrdValue::List(vec![text("OBJECTIVE24"), block])])
}

#[test]
fn accept_c4_m01_fu_the_stray_phrase_is_dropped_and_its_neighbours_bind() {
    let document = phrase_record();
    let record = measure_control_record(&document);
    let lowered = lower_control_record(
        ContentId::from_source(ContentKind::Mission, "accept-c4-m01-fu")
            .map_err(|error| error.to_string()),
        "accept-c4-m01-fu",
        &document,
        &record,
    );
    let attempt = lowered.attempt();
    assert_eq!(attempt.ignored_sites, IGNORED);
    assert_eq!(attempt.calls.len(), 2, "BEGIN_DORMANT and SET_AI_NET");
    assert!(
        attempt
            .calls
            .iter()
            .all(|call| matches!(call, CallOutcome::Bound)),
        "{:?}",
        attempt.calls
    );
}

#[test]
#[ignore = "requires CS_GAME_DIR"]
fn accept_c4_m01_fu_the_c4_m01_row_lowers_with_four_ignored_sites_and_is_complete() {
    let census = census();
    let row = census.row(MISSION).expect("the C4 M01 census row");
    let attempt = row.lowering_attempt().expect("a measured row lowers");
    let lowered = attempt.attempt();
    assert_eq!(lowered.ignored_sites, IGNORED);
    assert!(
        lowered
            .calls
            .iter()
            .all(|call| matches!(call, CallOutcome::Bound)),
        "{:?}",
        lowered.calls
    );
    assert!(
        lowered
            .conditions
            .iter()
            .all(|c| *c == ConditionOutcome::Lowered)
    );
    assert!(lowered.validation.is_some(), "the program was validated");
    assert!(row.is_complete());
    assert!(census.complete_missions().contains(&MISSION));
    assert!(!census.campaign_ready(), "other rows are still incomplete");
}
