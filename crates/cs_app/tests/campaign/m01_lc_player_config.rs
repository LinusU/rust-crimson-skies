//! M01-LC-PLAYER-CONFIG: the player and wingmate records of a mission's
//! `aiv.zrd` are bound with provenance, and the airframe and pose, which no
//! measured field carries, are named unknowns rather than guesses.

use std::path::PathBuf;

use cs_app::mission_start::{
    AIRFRAME_UNKNOWN_REASON, MissionStartConfiguration, POSE_UNKNOWN_REASON,
    recover_retail_start_configuration,
};
use cs_content::stunts::ZrdValue;
use cs_types::asset_id::SourceSpan;
use cs_types::content::Resolved;
use cs_types::evidence::{ClaimStatus, ContentHash};

fn text(value: &str) -> ZrdValue {
    ZrdValue::Text(value.to_owned())
}

fn record(name: &str, reference: &str) -> ZrdValue {
    let mut fields = vec![ZrdValue::Int(0); 8];
    fields[6] = text(reference);
    ZrdValue::List(vec![text(name), ZrdValue::List(fields)])
}

fn span() -> SourceSpan {
    SourceSpan::new(
        ContentHash::from_hex(&"ab".repeat(32)).expect("hash"),
        "ZBD/C0/M00/zrdr.zbd",
        Some("aiv.zrd"),
        16,
        64,
        None,
    )
    .expect("span")
}

#[test]
fn accept_m01_lc_player_config_binds_records_and_names_the_unknowns() {
    let document = ZrdValue::List(vec![
        ZrdValue::List(vec![ZrdValue::Int(1), text("Player")]),
        record("player", ""),
        record("devastator_3", ""),
        record("wingman_3", "Devastator_3"),
        record("wingman_x", "devastator_3"),
        record("wingman_4", "no_such_record"),
    ]);
    let config = MissionStartConfiguration::read("zbd/c0/m00", &document, &span()).expect("read");

    let Resolved::Known(player) = config.player() else {
        panic!("one player record is bound");
    };
    assert_eq!((player.value.index, player.value.field_count), (1, 8));
    assert_eq!(player.value.provenance.class, ClaimStatus::ObservedTool);
    assert_eq!(player.value.provenance.source.as_ref(), Some(&span()));
    assert_eq!(player.value.field_six_record, None);

    let names: Vec<_> = config.wingmates().iter().map(|w| w.name.as_str()).collect();
    assert_eq!(names, ["wingman_3", "wingman_4"]);
    assert_eq!(config.wingmates()[0].field_six_record, Some(2));
    assert_eq!(config.wingmates()[1].field_six_record, None);

    let Resolved::Unknown { reason, .. } = config.airframe() else {
        panic!("no airframe is measured");
    };
    assert_eq!(reason, AIRFRAME_UNKNOWN_REASON);
    assert_eq!(config.wingmate_airframes().len(), 2);
    assert!(config.wingmate_airframes().iter().all(|a| !a.is_known()));
    let Resolved::Unknown { reason, .. } = config.initial_pose() else {
        panic!("no pose is measured");
    };
    assert_eq!(reason, POSE_UNKNOWN_REASON);
}

#[test]
fn accept_m01_lc_player_config_refuses_a_missing_or_ambiguous_player() {
    for records in [vec![], vec![record("player", ""), record("Player", "")]] {
        let mut items = vec![ZrdValue::List(vec![ZrdValue::Int(0)])];
        items.extend(records);
        let config = MissionStartConfiguration::read("zbd/c0/m00", &ZrdValue::List(items), &span())
            .expect("read");
        assert!(!config.player().is_known());
    }
}

#[test]
#[ignore = "requires CS_GAME_DIR"]
fn accept_m01_lc_player_config_retail_m01_binds_player_and_names_unknowns() {
    let root = PathBuf::from(std::env::var("CS_GAME_DIR").expect("CS_GAME_DIR must be set"));
    let config = recover_retail_start_configuration(&root, "zbd/c1c/m01").expect("M01 reads");

    let Resolved::Known(player) = config.player() else {
        panic!("M01 has exactly one `player` record");
    };
    assert_eq!(player.value.name, "player");
    assert_eq!(player.value.index, 1);
    let source = player.value.provenance.source.as_ref().expect("a span");
    assert_eq!(source.member_key(), Some("aiv.zrd"));

    let names: Vec<_> = config.wingmates().iter().map(|w| w.name.as_str()).collect();
    assert_eq!(names, ["wingman_3", "wingman_2"]);
    // Both name a record of the same table (`devastator_3`, `devastator_2`).
    assert_eq!(config.wingmates()[0].field_six_record, Some(3));
    assert_eq!(config.wingmates()[1].field_six_record, Some(5));

    assert!(!config.airframe().is_known());
    assert!(!config.initial_pose().is_known());
}
