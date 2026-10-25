//! C5-M04-FU (#2135): `zbd/c5/m04` objective 19 spells the truncated key
//! `SET_AI_`; the original's parser looks up only `SET_AI_TEAM`,
//! `SET_AI_ATTACK_RADIUS` and `SET_AI_NET` by whole-string compare
//! (`docs/findings/2026-10-25-c5-m04-fu-truncated-set-ai.md`), so the adapter
//! drops the site and the row is complete. Retail tests are
//! `#[ignore = "requires CS_GAME_DIR"]`.

use std::path::PathBuf;
use std::sync::OnceLock;

use cs_app::control_lowering::lower_control_record;
use cs_app::mission_control::{RetailControlCensus, survey_mission_control_programs};
use cs_content::mission_control::{CallOutcome, ConditionOutcome, measure_control_record};
use cs_content::stunts::ZrdValue;
use cs_types::content::{ContentId, ContentKind};

const MISSION: &str = "zbd/c5/m04";
const IGNORED: u32 = 1;

fn census() -> &'static RetailControlCensus {
    static CENSUS: OnceLock<RetailControlCensus> = OnceLock::new();
    CENSUS.get_or_init(|| {
        let dir = std::env::var("CS_GAME_DIR").unwrap_or_else(|_| {
            panic!("CS_GAME_DIR is not set: C5-M04-FU needs the retail capability")
        });
        survey_mission_control_programs(&PathBuf::from(dir))
            .expect("the census runs on the installation")
    })
}

fn text(value: &str) -> ZrdValue {
    ZrdValue::Text(value.to_owned())
}

/// Objective 19 as spelled: the truncated key and its pair list.
fn truncated_record() -> ZrdValue {
    let pair = |name: &str| ZrdValue::List(vec![text(name), text("M4Defend")]);
    let block = ZrdValue::List(vec![
        text("BEGIN_DORMANT"),
        ZrdValue::List(vec![ZrdValue::Float(-1.0)]),
        text("SET_AI_"),
        ZrdValue::List(vec![pair("devastator_1"), pair("wingman_1")]),
    ]);
    ZrdValue::List(vec![ZrdValue::List(vec![text("OBJECTIVE19"), block])])
}

#[test]
fn accept_c5_m04_fu_the_truncated_key_is_dropped_with_its_list() {
    let document = truncated_record();
    let record = measure_control_record(&document);
    let lowered = lower_control_record(
        ContentId::from_source(ContentKind::Mission, "accept-c5-m04-fu")
            .map_err(|error| error.to_string()),
        "accept-c5-m04-fu",
        &document,
        &record,
    );
    let attempt = lowered.attempt();
    assert_eq!(attempt.ignored_sites, IGNORED);
    assert_eq!(attempt.calls.len(), 1, "BEGIN_DORMANT only");
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
fn accept_c5_m04_fu_the_c5_m04_row_lowers_with_one_ignored_site_and_is_complete() {
    let census = census();
    let row = census.row(MISSION).expect("the C5 M04 census row");
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
