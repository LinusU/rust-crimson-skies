//! M14-B-FU1: M14's control program lowers once `TRAVELERS` accepts an int
//! radius (`OBJECTIVE25` spells `1000`; the original reads child 3 by type
//! tag, `0x467a70`). Retail tests are `#[ignore = "requires CS_GAME_DIR"]`.

use std::path::PathBuf;
use std::sync::OnceLock;

use cs_app::mission_control::{RetailControlCensus, survey_mission_control_programs};
use cs_content::mission_control::{CallOutcome, ConditionOutcome};

const MISSION: &str = "zbd/c2b/m04";
const CALLS: usize = 254;
const CONDITIONS: usize = 35;

fn census() -> &'static RetailControlCensus {
    static CENSUS: OnceLock<RetailControlCensus> = OnceLock::new();
    CENSUS.get_or_init(|| {
        let dir = std::env::var("CS_GAME_DIR").unwrap_or_else(|_| {
            panic!("CS_GAME_DIR is not set: M14-B-FU1 needs the retail capability")
        });
        survey_mission_control_programs(&PathBuf::from(dir))
            .expect("the census runs on the installation")
    })
}

#[test]
#[ignore = "requires CS_GAME_DIR"]
fn accept_m14_b_fu1_block_25_lowers_and_the_m14_row_is_complete() {
    let census = census();
    let row = census.row(MISSION).expect("M14's census row");
    let attempt = row.lowering_attempt().expect("a measured row lowers");
    let lowered = attempt.attempt();
    assert_eq!(lowered.calls.len(), CALLS);
    assert!(
        lowered
            .calls
            .iter()
            .all(|c| matches!(c, CallOutcome::Bound))
    );
    assert_eq!(lowered.conditions.len(), CONDITIONS);
    assert!(
        lowered
            .conditions
            .iter()
            .all(|c| *c == ConditionOutcome::Lowered),
        "{:?}",
        lowered.conditions
    );
    assert!(lowered.validation.is_some(), "the program was validated");
    assert!(attempt.program().is_some());
    assert!(row.lowering().unwrap().complete());
    assert!(row.is_complete());
    assert!(census.complete_missions().contains(&MISSION));
    assert!(
        !census.campaign_ready(),
        "other rows are still incomplete, so the gate stays closed"
    );
}

/// The other incomplete row holds on a different cause: a host call whose
/// name the reader truncated, not a TRAVELERS radius. (`zbd/c4/m01` held on an
/// unquoted phrase until #2134, which the original ignores.)
#[test]
#[ignore = "requires CS_GAME_DIR"]
fn accept_m14_b_fu1_other_incomplete_rows_do_not_fail_on_the_int_radius() {
    let census = census();
    let (mission, refused_call) = ("zbd/c5/m04", "unknown host call `SET_AI_`");
    let row = census.row(mission).expect("a census row");
    assert!(!row.is_complete(), "{mission}");
    let lowered = row.lowering_attempt().expect("measured").attempt();
    assert!(
        lowered
            .conditions
            .iter()
            .all(|c| *c == ConditionOutcome::Lowered),
        "{mission}: every condition lowers, so the radius is not the cause"
    );
    assert!(
        lowered
            .calls
            .iter()
            .any(|c| matches!(c, CallOutcome::Refused(why) if why.contains(refused_call))),
        "{mission}: the refused call is {refused_call}"
    );
}
