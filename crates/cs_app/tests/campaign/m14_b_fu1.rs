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

/// The other row that held at the census (`zbd/c5/m04`, a truncated
/// `SET_AI_` key) has a different cause from the int radius, and the original
/// ignores that key (#2135): it is complete now, with one dropped site.
/// (`zbd/c4/m01` held on an unquoted phrase until #2134.)
#[test]
#[ignore = "requires CS_GAME_DIR"]
fn accept_m14_b_fu1_other_formerly_incomplete_rows_were_not_the_int_radius() {
    let census = census();
    let mission = "zbd/c5/m04";
    let row = census.row(mission).expect("a census row");
    let lowered = row.lowering_attempt().expect("measured").attempt();
    assert!(
        lowered
            .conditions
            .iter()
            .all(|c| *c == ConditionOutcome::Lowered),
        "{mission}: every condition lowers"
    );
    assert_eq!(lowered.ignored_sites, 1, "{mission}: the truncated key");
    assert!(row.is_complete(), "{mission}");
}
