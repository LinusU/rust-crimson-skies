//! M01-LC-CAMPAIGN-AIRFRAME-POSE (#770): the two values no mission document
//! carries — the campaign player's airframe and the metric start pose — are
//! bound from measured engine state with the source named, and refused by
//! name when this installation cannot name it.
//!
//! The evidence is static analysis of the owner-supplied decrypted image
//! (`$CS_GAME_DIR/crimson.decrypted.exe`, sha256 as
//! [`ENGINE_IMAGE_SHA256`]) plus retail data, recorded in
//! `docs/findings/2026-10-08-m01-lc-campaign-airframe-engine-state.md`.
//! Nothing here is `verified_original`: no original run supplied any of it,
//! so every binding is [`ClaimStatus::ObservedTool`].
//!
//! Two things are pinned so a reviewer can see the binding is not circular:
//!
//! * the airframe's row is read back out of the image's own bytes at
//!   [`CAMPAIGN_AIRFRAME_RECORD_OFFSET`], and the degree-to-radian constant
//!   out of the bytes at [`HEADING_DEGREES_CONSTANT_OFFSET`], instead of
//!   being asserted against the constants that were transcribed from them;
//! * the retail suite binds M01 through
//!   [`recover_retail_start_configuration`] and checks the provenance spans
//!   name those bytes.

use std::path::PathBuf;

use cs_app::mission_start::{
    AIRFRAME_TABLE, AIRFRAME_UNKNOWN_REASON, CAMPAIGN_AIRFRAME_RECORD_LENGTH,
    CAMPAIGN_AIRFRAME_RECORD_OFFSET, CAMPAIGN_AIRFRAME_ROW, CAMPAIGN_AIRFRAME_SOURCE, ENGINE_IMAGE,
    ENGINE_IMAGE_SHA256, EngineStateError, EngineStateSource, HEADING_CONVERSION_LENGTH,
    HEADING_CONVERSION_OFFSET, HEADING_DEGREES_CONSTANT_LENGTH, HEADING_DEGREES_CONSTANT_OFFSET,
    MissionStartConfiguration, POSE_UNKNOWN_REASON, STORED_HEADING_DEGREES_TO_RADIANS,
    engine_state_source, recover_retail_start_configuration, stored_heading_radians,
};
use cs_content::stunts::ZrdValue;
use cs_types::asset_id::SourceSpan;
use cs_types::content::{ContentId, ContentKind, Resolved};
use cs_types::evidence::{ClaimStatus, ContentHash};
use cs_types::install::{FileRole, InstallFileRecord, InstallManifest, ParseState, RelativePath};

/// The digest the measured image carries, as a [`ContentHash`].
fn image_hash() -> ContentHash {
    ContentHash::from_hex(ENGINE_IMAGE_SHA256).expect("the recorded image digest is hex")
}

/// An installation inventory carrying `crimson.decrypted.exe` at `digest`.
fn inventory(digest: ContentHash) -> InstallManifest {
    InstallManifest::new(
        PathBuf::from("/game"),
        vec![InstallFileRecord {
            relative_spelling: RelativePath::new(ENGINE_IMAGE).expect("a plain file name"),
            size_bytes: 2_580_480,
            sha256: digest,
            family: None,
            role: FileRole::Unknown,
            parse_state: ParseState::Unparsed,
        }],
    )
    .expect("one row is a valid inventory")
}

fn text(value: &str) -> ZrdValue {
    ZrdValue::Text(value.to_owned())
}

/// A player record with M01's measured stored pose (#676).
fn player_document() -> ZrdValue {
    let mut fields = vec![ZrdValue::Int(0); 8];
    fields[0] = ZrdValue::Int(u32::MAX);
    fields[1] = ZrdValue::List(vec![
        ZrdValue::Float(-3694.0),
        ZrdValue::Float(1318.0),
        ZrdValue::Float(-12482.0),
    ]);
    fields[2] = ZrdValue::Float(170.0);
    ZrdValue::List(vec![
        ZrdValue::List(vec![ZrdValue::Int(1)]),
        ZrdValue::List(vec![text("player"), ZrdValue::List(fields)]),
    ])
}

fn span(offset: u64, length: u64) -> SourceSpan {
    SourceSpan::new(image_hash(), ENGINE_IMAGE, None, offset, length, None).expect("a span")
}

fn engine_source() -> EngineStateSource {
    EngineStateSource {
        airframe: span(
            CAMPAIGN_AIRFRAME_RECORD_OFFSET,
            CAMPAIGN_AIRFRAME_RECORD_LENGTH,
        ),
        heading: span(HEADING_CONVERSION_OFFSET, HEADING_CONVERSION_LENGTH),
    }
}

#[test]
fn accept_m01_lc_campaign_airframe_pose_engine_state_source_names_the_measured_bytes() {
    // The measured image: the inventory row's own digest is the one the
    // binding names, so a drifted image can never back it.
    let found = engine_state_source(&inventory(image_hash())).expect("the measured image names");
    assert_eq!(found.airframe.container_path(), ENGINE_IMAGE);
    assert_eq!(
        found.airframe.member_key(),
        None,
        "a loose file, not a container"
    );
    assert_eq!(found.airframe.install_sha256(), image_hash());
    assert_eq!(
        (found.airframe.offset(), found.airframe.length()),
        (
            CAMPAIGN_AIRFRAME_RECORD_OFFSET,
            CAMPAIGN_AIRFRAME_RECORD_LENGTH
        )
    );
    assert_eq!(
        (found.heading.offset(), found.heading.length()),
        (HEADING_CONVERSION_OFFSET, HEADING_CONVERSION_LENGTH)
    );

    // An installation without that image says so instead of binding.
    let absent = InstallManifest::new(PathBuf::from("/game"), vec![]).expect("an empty inventory");
    let error = engine_state_source(&absent).expect_err("no image, no source");
    assert_eq!(error, EngineStateError::ImageAbsent);
    assert!(error.to_string().contains(ENGINE_IMAGE));

    // So does one whose image is not the measured bytes.
    let other = ContentHash::from_hex(&"cd".repeat(32)).expect("hex");
    let error = engine_state_source(&inventory(other)).expect_err("a different image");
    assert_eq!(error, EngineStateError::DigestMismatch { found: other });
    assert!(error.to_string().contains(ENGINE_IMAGE_SHA256));
}

#[test]
fn accept_m01_lc_campaign_airframe_pose_bind_names_the_source_and_keeps_the_refusals() {
    let mut config =
        MissionStartConfiguration::read("zbd/c1c/m00", &player_document(), &span(16, 64))
            .expect("read");

    // From the document alone nothing is bound — the refusals stay.
    let Resolved::Unknown { reason, .. } = config.airframe() else {
        panic!("the document alone assigns no airframe");
    };
    assert_eq!(reason, AIRFRAME_UNKNOWN_REASON);
    let Resolved::Unknown { reason, .. } = config.initial_pose() else {
        panic!("the document alone carries no conversion");
    };
    assert_eq!(reason, POSE_UNKNOWN_REASON);

    // With the measured source named, both become Known.
    let engine = engine_source();
    config.bind_engine_state(&engine).expect("bind");

    let Resolved::Known(airframe) = config.airframe() else {
        panic!("the campaign airframe is measured");
    };
    assert_eq!(
        airframe.value,
        ContentId::from_source(
            ContentKind::Airframe,
            AIRFRAME_TABLE[CAMPAIGN_AIRFRAME_ROW].scene_root
        )
        .expect("a scene-root key is a valid id")
    );
    assert_eq!(
        (
            airframe.value.key(),
            AIRFRAME_TABLE[CAMPAIGN_AIRFRAME_ROW].display_name
        ),
        ("player_pfighter", "Devastator")
    );
    assert_eq!(airframe.provenance.class, ClaimStatus::ObservedTool);
    assert_eq!(airframe.provenance.source.as_ref(), Some(&engine.airframe));

    let Resolved::Known(pose) = config.initial_pose() else {
        panic!("the pose's convention is measured");
    };
    assert_eq!(pose.value.position, [-3694.0, 1318.0, -12482.0]);
    assert_eq!(pose.value.heading, stored_heading_radians(170.0));
    assert_eq!(pose.provenance.class, ClaimStatus::ObservedTool);
    assert_eq!(pose.provenance.source.as_ref(), Some(&engine.heading));

    // A refusal never withdraws a binding, and the source's own prose names
    // the profile/flight-check shape and its residues.
    config
        .refuse_engine_state("the inventory carries no measured image")
        .expect("refusal recorded");
    assert!(config.airframe().is_known());
    assert!(config.initial_pose().is_known());
    for needle in [
        "profile/flight-check",
        "row 5 Devastator",
        "crimson.decrypted.exe",
        "F13-B/C, F38",
    ] {
        assert!(
            CAMPAIGN_AIRFRAME_SOURCE.contains(needle),
            "the airframe source must name {needle:?}"
        );
    }
}

#[test]
fn accept_m01_lc_campaign_airframe_pose_a_record_without_a_pose_stays_unknown() {
    let mut shapeless = vec![ZrdValue::Int(0); 8];
    shapeless[1] = ZrdValue::List(vec![ZrdValue::Float(1.0), ZrdValue::Float(2.0)]);
    let document = ZrdValue::List(vec![
        ZrdValue::List(vec![ZrdValue::Int(0)]),
        ZrdValue::List(vec![text("player"), ZrdValue::List(shapeless)]),
    ]);
    let mut config =
        MissionStartConfiguration::read("zbd/c1c/m00", &document, &span(16, 64)).expect("read");
    config.bind_engine_state(&engine_source()).expect("bind");

    assert!(
        config.airframe().is_known(),
        "the airframe does not depend on the record's pose"
    );
    let Resolved::Unknown { reason, .. } = config.initial_pose() else {
        panic!("nothing to convert, and none invented");
    };
    assert_eq!(reason, POSE_UNKNOWN_REASON);
}

#[test]
fn accept_m01_lc_campaign_airframe_pose_stored_heading_converts_like_the_original() {
    // The constant is the image's own double (its bytes are checked by the
    // retail test), and the conversion rounds exactly as the original does:
    // f32 degrees widened, multiplied by a f64, rounded back to f32.
    assert_eq!(STORED_HEADING_DEGREES_TO_RADIANS, 0.01745329251994);
    assert_eq!(HEADING_DEGREES_CONSTANT_LENGTH, 8);

    assert_eq!(stored_heading_radians(0.0), 0.0);
    let turns = stored_heading_radians(360.0);
    assert!((turns - std::f32::consts::TAU).abs() < 1e-5, "{turns}");
    let half = stored_heading_radians(180.0);
    assert!((half - std::f32::consts::PI).abs() < 1e-5, "{half}");
    // M01's stored heading (#676), the value the binding hands the player.
    let m01 = stored_heading_radians(170.0);
    assert_eq!(m01, 2.9670596);
    // ... and it is the image's conversion, not an f32 shortcut that would
    // disagree in the last bit for some angles.
    let shortcut = 170.0f32.to_radians();
    assert!(
        (shortcut - m01).abs() <= f32::EPSILON,
        "{shortcut} vs {m01}"
    );
}

#[test]
#[ignore = "requires CS_GAME_DIR"]
fn accept_m01_lc_campaign_airframe_pose_retail_m01_binds_from_the_measured_bytes() {
    let root = PathBuf::from(std::env::var("CS_GAME_DIR").expect("CS_GAME_DIR must be set"));

    let found = cs_assets::install::discover(&root).expect("the installation discovers");
    let engine = engine_state_source(&found.manifest).expect("the measured image is inventoried");
    assert_eq!(
        (engine.airframe.offset(), engine.heading.offset()),
        (CAMPAIGN_AIRFRAME_RECORD_OFFSET, HEADING_CONVERSION_OFFSET)
    );

    // Read the two bindings back out of the image's own bytes: the roster
    // record's airframe row, and the degree-to-radian double.
    let image = std::fs::read(root.join(ENGINE_IMAGE)).expect("the image reads");
    let row_offset = (CAMPAIGN_AIRFRAME_RECORD_OFFSET + 0x2c) as usize;
    let row = u32::from_le_bytes(
        image[row_offset..row_offset + 4]
            .try_into()
            .expect("four bytes for the row"),
    );
    assert_eq!(
        row as usize, CAMPAIGN_AIRFRAME_ROW,
        "the roster record the campaign start copies carries row {CAMPAIGN_AIRFRAME_ROW}"
    );
    let constant = f64::from_le_bytes(
        image[HEADING_DEGREES_CONSTANT_OFFSET as usize
            ..HEADING_DEGREES_CONSTANT_OFFSET as usize + HEADING_DEGREES_CONSTANT_LENGTH as usize]
            .try_into()
            .expect("eight bytes for the double"),
    );
    assert_eq!(
        constant, STORED_HEADING_DEGREES_TO_RADIANS,
        "the image's π/180 double is the one the binding converts with"
    );

    // The production reader, end to end.
    let config = recover_retail_start_configuration(&root, "zbd/c1c/m01").expect("M01 reads");

    let Resolved::Known(airframe) = config.airframe() else {
        panic!("M01's campaign airframe is measured engine state");
    };
    assert_eq!(
        airframe.value.key(),
        AIRFRAME_TABLE[CAMPAIGN_AIRFRAME_ROW].scene_root
    );
    assert_eq!(
        (
            airframe.value.key(),
            AIRFRAME_TABLE[CAMPAIGN_AIRFRAME_ROW].model
        ),
        ("player_pfighter", "piratefighter")
    );
    assert_eq!(airframe.provenance.class, ClaimStatus::ObservedTool);
    assert_eq!(airframe.provenance.source.as_ref(), Some(&engine.airframe));

    let Resolved::Known(pose) = config.initial_pose() else {
        panic!("M01's start pose is a decoded binding through a measured convention");
    };
    assert_eq!(pose.value.position, [-3694.0, 1318.0, -12482.0]);
    assert_eq!(pose.value.heading, stored_heading_radians(170.0));
    assert_eq!(pose.provenance.class, ClaimStatus::ObservedTool);
    assert_eq!(pose.provenance.source.as_ref(), Some(&engine.heading));

    // The stored pose is still bound on its own terms.
    let Resolved::Known(stored) = config.stored_pose() else {
        panic!("M01's player record has the pose shape");
    };
    assert_eq!(stored.value.heading, 170.0);
}

#[test]
#[ignore = "requires CS_GAME_DIR"]
fn accept_m01_lc_campaign_airframe_pose_an_instant_action_scenario_keeps_its_own_assignment() {
    let root = PathBuf::from(std::env::var("CS_GAME_DIR").expect("CS_GAME_DIR must be set"));

    // The same reader, pointed at this chapter's instant-action archive: it
    // carries both `aiv.zrd` and an `ia.zrd` whose `player_plane` assigns the
    // player (#715 measured the key's eight archives and no campaign one), so
    // mode 3 reads that assignment and the campaign engine-state chain is not
    // the chain that decides — the airframe must stay refused under the
    // scenario's own name rather than be bound to the campaign default.
    let config = recover_retail_start_configuration(&root, "zbd/c1c/ia1")
        .expect("the chapter's instant-action archive reads");
    let Resolved::Unknown { reason, .. } = config.airframe() else {
        panic!(
            "an instant-action scenario's own assignment must not be overridden by the campaign chain"
        );
    };
    for needle in ["ia.zrd", "player_plane", "Fury", "instant-action"] {
        assert!(
            reason.contains(needle),
            "the refusal must name {needle:?}: {reason}"
        );
    }
    assert!(
        config.stored_pose().is_known(),
        "the record's stored pose is read whatever the mode"
    );
}
