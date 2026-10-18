//! M01-LC-PLAYER-CONFIG: the player and wingmate records of a mission's
//! `aiv.zrd` are bound with provenance, and the airframe and pose, which no
//! measured field carries, are named unknowns rather than guesses.

use std::path::PathBuf;

use crate::campaign::common::label;
use avian3d::prelude::Position;
use bevy::prelude::With;
use cs_app::mission_launch::plan_mission_launch;
use cs_app::mission_session::{MissionPlayerBody, MissionPlayerStart, build_headless, stage_for};
use cs_app::mission_start::{
    AIRFRAME_UNKNOWN_REASON, MissionStartConfiguration, POSE_UNKNOWN_REASON,
    recover_retail_start_configuration, stored_heading_radians,
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
    fields[0] = ZrdValue::Int(u32::MAX);
    fields[1] = ZrdValue::List(vec![
        ZrdValue::Float(-3694.0),
        ZrdValue::Float(1318.0),
        ZrdValue::Float(-12482.0),
    ]);
    fields[2] = ZrdValue::Float(170.0);
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
        panic!("no metric pose is measured");
    };
    assert_eq!(reason, POSE_UNKNOWN_REASON);

    assert_eq!(player.value.field_zero, Some(u32::MAX));
    let Resolved::Known(stored) = config.stored_pose() else {
        panic!("the stored pose is bound");
    };
    assert_eq!(stored.value.position, [-3694.0, 1318.0, -12482.0]);
    assert_eq!(stored.value.heading, 170.0);
    assert_eq!(stored.provenance.class, ClaimStatus::ObservedTool);
    assert_eq!(stored.provenance.source.as_ref(), Some(&span()));
}

#[test]
fn accept_m01_lc_player_config_a_player_without_the_pose_shape_has_no_stored_pose() {
    let mut shapeless = vec![ZrdValue::Int(0); 8];
    shapeless[1] = ZrdValue::List(vec![ZrdValue::Float(1.0), ZrdValue::Float(2.0)]);
    let document = ZrdValue::List(vec![
        ZrdValue::List(vec![ZrdValue::Int(0)]),
        ZrdValue::List(vec![text("player"), ZrdValue::List(shapeless)]),
    ]);
    let config = MissionStartConfiguration::read("zbd/c0/m00", &document, &span()).expect("read");
    assert!(config.player().is_known());
    assert!(!config.stored_pose().is_known());
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

/// The `zbd/c1c/m01` archive is ch2-m01, M06 "The Red Menace" (#1565's
/// corrected layout): these are M06's measured record values, kept as M06
/// measurements. The image-backed leg binds the measured fresh-profile
/// default for any mission dir, so the airframe stays Known here.
#[test]
#[ignore = "requires CS_GAME_DIR and CS_ENGINE_IMAGE"]
fn accept_m01_lc_player_config_retail_m06_binds_player_and_names_unknowns() {
    let root = PathBuf::from(std::env::var("CS_GAME_DIR").expect("CS_GAME_DIR must be set"));
    let config = recover_retail_start_configuration(&root, "zbd/c1c/m01").expect("M06 reads");

    let Resolved::Known(player) = config.player() else {
        panic!("M06 has exactly one `player` record");
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

    // #770: with this installation carrying the image the chain was measured
    // in, the airframe and the metric pose are bound with a named source.
    let Resolved::Known(airframe) = config.airframe() else {
        panic!("the campaign airframe is measured engine state");
    };
    assert_eq!(airframe.value.key(), "player_pfighter");
    let Resolved::Known(pose) = config.initial_pose() else {
        panic!("the start pose is a decoded binding through a measured convention");
    };
    assert_eq!(pose.value.position, [-3694.0, 1318.0, -12482.0]);
    assert_eq!(pose.value.heading, stored_heading_radians(170.0));

    // Measured values of M06's player record (and no airframe in field 0).
    assert_eq!(player.value.field_zero, Some(u32::MAX));
    assert!(
        config
            .wingmates()
            .iter()
            .all(|w| w.field_zero == Some(u32::MAX))
    );
    let Resolved::Known(stored) = config.stored_pose() else {
        panic!("M06's player record has the pose shape");
    };
    assert_eq!(stored.value.position, [-3694.0, 1318.0, -12482.0]);
    assert_eq!(stored.value.heading, 170.0);
    assert_eq!(
        config.wingmates()[0].stored_pose.map(|p| p.position),
        Some([-4531.0, 1280.0, -13065.0])
    );
}

/// **The staged start pose is the `player` record of `aiv.zrd`, and nothing
/// else.**
///
/// This stage composes the `zbd/c1c/m01` mission — ch2-m01, M06 "The Red
/// Menace" under #1565's corrected layout — whose pose comes from
/// `ZBD/C1C/M01/zrdr.zbd` → member `aiv.zrd` → the record named `player`
/// (document index 1): field 1 (`POSITION_FIELD`, three floats, metres, axis
/// 1 vertical) and field 2 (`HEADING_FIELD`, degrees-like, turned into
/// radians by `stored_heading_radians`). The composition's stage, its player
/// spawn recipe and the live body must all equal what the production reader
/// returns for that record, and the record's source span names its bytes in
/// the member.
///
/// Composing the stage also binds the player's airframe: `zbd/c1c/m01` is a
/// progression-reached mission, so the committed table keeps it an explicit
/// unknown (#1580) and only the engine image supplies the measured default.
#[test]
#[ignore = "requires CS_GAME_DIR and CS_ENGINE_IMAGE"]
fn accept_m01_window_world_start_pose_is_the_aiv_zrd_player_record() {
    let root = PathBuf::from(std::env::var("CS_GAME_DIR").expect("CS_GAME_DIR must be set"));
    let config = recover_retail_start_configuration(&root, "zbd/c1c/m01").expect("M06 reads");

    // The record the numbers come from, with the span of its bytes.
    let Resolved::Known(player) = config.player() else {
        panic!("M06 has exactly one `player` record");
    };
    assert_eq!(
        (player.value.name.as_str(), player.value.index),
        ("player", 1)
    );
    let source = player.value.provenance.source.as_ref().expect("a span");
    assert_eq!(source.member_key(), Some("aiv.zrd"));
    assert!(
        source
            .container_path()
            .eq_ignore_ascii_case("ZBD/C1C/M01/zrdr.zbd"),
        "container {}",
        source.container_path()
    );
    assert!(
        source.length() > 0,
        "the record names a non-empty byte range"
    );
    let Resolved::Known(stored) = config.stored_pose() else {
        panic!("M06's player record has the pose shape");
    };
    assert_eq!(stored.provenance.source.as_ref(), Some(source));
    let expected_position = stored.value.position_metres();
    let expected_heading = stored_heading_radians(stored.value.heading);
    // Altitude is field 1's second float, not a value from prose.
    assert_eq!(expected_position, [-3694.0, 1318.0, -12482.0]);
    assert_eq!(stored.value.heading, 170.0);

    // The composition's stage, spawn recipe and live body carry that pose.
    let plan = plan_mission_launch(&root, label("M06"), "The Red Menace")
        .expect("M06's launch closure plans");
    let stage = stage_for(&root, &plan).expect("M06's stage reads");
    assert_eq!(stage.start.position, expected_position);
    assert_eq!(stage.start.heading, expected_heading);

    let mut app = build_headless(&stage).expect("M06's stage composes headlessly");
    app.update();
    let recipe = app.world().resource::<MissionPlayerStart>();
    assert_eq!(recipe.position_m, expected_position);
    let world = app.world_mut();
    let bodies: Vec<_> = world
        .query_filtered::<&Position, With<MissionPlayerBody>>()
        .iter(world)
        .map(|position| position.0.to_array())
        .collect();
    assert_eq!(bodies.len(), 1, "exactly one player body");
    // The body has not been flown: one update from the recipe's own start.
    for (live, recorded) in bodies[0].iter().zip(expected_position) {
        assert!((live - recorded).abs() < 1.0, "{live} vs {recorded}");
    }
}
