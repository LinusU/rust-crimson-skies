//! F40-D3 acceptance: the measured `cam_anim.zbd` event grammar, the camera
//! cinematics it carries, and the explicit unknowns the content half declares.
//!
//! Spec: `specs/F40-cutscenes-video-scripted-cameras-and-transitions.md`,
//! stage `### F40-D`. Finding:
//! `docs/findings/2026-10-13-f40-d3-cam-anim-camera-format.md`.
//!
//! The synthetic tests build event bytes by hand and walk them through the
//! production parser; the two retail tests re-measure from `$CS_GAME_DIR` on
//! every run — the eight camera carriers, and the full camera-cinematic
//! inventory over every animation carrier (58 records in 31 carriers) — and
//! are ignored on machines without the original installation.
//!
//! Run the retail half with:
//!
//! ```sh
//! cargo test --workspace --locked -- accept_f40_d3_ --include-ignored
//! ```

use cs_app::cinematics::camera_anim::{
    CAMERA_EVENT_HEADER_BYTES, CAMERA_OBJECT_NAME, CameraAnimStreamError, MEASURED_MAX_EVENT_BYTES,
    MEASURED_MIN_EVENT_BYTES, MeasuredEventSize, measured_event_total_size,
    parse_camera_anim_stream,
};
use cs_app::cinematics::{CinematicLowerError, lower_cinematic};
use cs_content::cinematics::{DeclaredPresentation, declared_camera_sequence};
use cs_types::content::{ContentId, ContentKind, Origin, Provenance};
use cs_types::evidence::ClaimId;
use cs_types::install::RelativePath;
use std::collections::{BTreeMap, BTreeSet};

/// Builds one TLV event: the 8-byte header and `payload`, padded to
/// `total_bytes` with zeros (the padding is never read by the walk).
fn event_bytes(type_id: u32, total_bytes: u32, payload: &[u8]) -> Vec<u8> {
    let mut bytes = Vec::new();
    bytes.extend_from_slice(&type_id.to_le_bytes());
    bytes.extend_from_slice(&total_bytes.to_le_bytes());
    bytes.extend_from_slice(payload);
    bytes.resize(total_bytes as usize, 0);
    bytes
}

#[test]
fn accept_f40_d3_event_stream_parses_as_the_measured_tlv() {
    // Three retail-shaped events: 0x120 (12 bytes: header + one zero word),
    // 0x123 (16 bytes: header + two words), 0x106 (20 bytes).
    let mut stream = Vec::new();
    stream.extend_from_slice(&event_bytes(0x120, 12, &[0, 0, 0, 0]));
    stream.extend_from_slice(&event_bytes(0x123, 16, &[0, 0, 0, 0, 5, 0, 0, 0]));
    stream.extend_from_slice(&event_bytes(
        0x106,
        20,
        &[0, 0, 0, 0, 0, 0, 0, 0, 1, 0, 0, 0],
    ));

    let events = parse_camera_anim_stream(&stream).expect("the stream tiles exactly");
    assert_eq!(events.len(), 3);
    assert_eq!(events[0].type_id(), 0x120);
    assert_eq!(events[0].total_bytes(), 12);
    assert_eq!(events[0].payload(), &[0, 0, 0, 0]);
    assert_eq!(events[1].type_id(), 0x123);
    assert_eq!(events[1].payload(), &[0, 0, 0, 0, 5, 0, 0, 0]);
    assert_eq!(events[2].type_id(), 0x106);
    assert_eq!(events[2].total_bytes(), 20);
    assert_eq!(events[2].payload().len(), 20 - CAMERA_EVENT_HEADER_BYTES);
    // Every event matches the measured fixed size of its type.
    assert!(events.iter().all(|event| event.matches_measured_size()));
    // An empty stream yields no events rather than an error (measured: retail
    // never stores one, but the walk is total).
    assert!(parse_camera_anim_stream(&[]).unwrap().is_empty());
}

#[test]
fn accept_f40_d3_event_stream_refuses_broken_streams_by_name() {
    // A header that does not fit.
    assert_eq!(
        parse_camera_anim_stream(&[1, 2, 3, 4, 5, 6, 7]).unwrap_err(),
        CameraAnimStreamError::TruncatedHeader {
            offset: 0,
            needed: 8,
            available: 7
        }
    );
    // A declared size below every retail event's minimum (8 < 12).
    let mut below = event_bytes(0x120, 12, &[0, 0, 0, 0]);
    below[4..8].copy_from_slice(&8u32.to_le_bytes());
    assert_eq!(
        parse_camera_anim_stream(&below).unwrap_err(),
        CameraAnimStreamError::SizeBelowMeasuredMinimum {
            offset: 0,
            type_id: 0x120,
            total_bytes: 8
        }
    );
    // An event that runs past its stream's end: the header declares 16 bytes,
    // the stream holds 12.
    let mut over = Vec::new();
    over.extend_from_slice(&0x123u32.to_le_bytes());
    over.extend_from_slice(&16u32.to_le_bytes());
    over.extend_from_slice(&[0, 0, 0, 0]);
    assert_eq!(
        parse_camera_anim_stream(&over).unwrap_err(),
        CameraAnimStreamError::EventOverrunsStream {
            offset: 0,
            type_id: 0x123,
            total_bytes: 16,
            available: 12
        }
    );
    // A good event followed by stray bytes: the leftover header refuses by
    // name rather than being skipped.
    let mut stray = event_bytes(0x120, 12, &[0, 0, 0, 0]);
    stray.extend_from_slice(&[0xAA, 0xBB, 0xCC]);
    assert_eq!(
        parse_camera_anim_stream(&stray).unwrap_err(),
        CameraAnimStreamError::TruncatedHeader {
            offset: 12,
            needed: 8,
            available: 3
        }
    );
    // Every refusal carries a stable code.
    for code in [
        "truncated_header",
        "size_below_measured_minimum",
        "event_overruns_stream",
    ] {
        let error = match code {
            "truncated_header" => CameraAnimStreamError::TruncatedHeader {
                offset: 0,
                needed: 8,
                available: 0,
            },
            "size_below_measured_minimum" => CameraAnimStreamError::SizeBelowMeasuredMinimum {
                offset: 0,
                type_id: 0,
                total_bytes: 0,
            },
            _ => CameraAnimStreamError::EventOverrunsStream {
                offset: 0,
                type_id: 0,
                total_bytes: 0,
                available: 0,
            },
        };
        assert_eq!(error.code(), code);
        assert!(!error.to_string().is_empty());
    }
}

#[test]
fn accept_f40_d3_measured_size_table_pins_the_retail_shapes() {
    assert_eq!(
        measured_event_total_size(0x120),
        Some(MeasuredEventSize::Fixed(12))
    );
    assert_eq!(
        measured_event_total_size(0x123),
        Some(MeasuredEventSize::Fixed(16))
    );
    assert_eq!(
        measured_event_total_size(0x10a),
        Some(MeasuredEventSize::Fixed(332))
    );
    assert_eq!(
        measured_event_total_size(0x12a),
        Some(MeasuredEventSize::Fixed(596))
    );
    assert_eq!(
        measured_event_total_size(0x129),
        Some(MeasuredEventSize::Fixed(36))
    );
    assert_eq!(
        measured_event_total_size(0x30c),
        Some(MeasuredEventSize::Fixed(76))
    );
    // The one dual-shaped type: both sizes measured, neither guessed.
    assert_eq!(
        measured_event_total_size(0x12f),
        Some(MeasuredEventSize::OneOf(548, 1156))
    );
    let dual = measured_event_total_size(0x12f).unwrap();
    assert!(dual.contains(548));
    assert!(dual.contains(1156));
    assert!(!dual.contains(552));
    // An unmeasured type stays unmeasured and never conforms.
    assert_eq!(measured_event_total_size(0x999), None);
    let stray_type = event_bytes(0x999, 12, &[0, 0, 0, 0]);
    let unmeasured = parse_camera_anim_stream(&stray_type).unwrap();
    assert!(!unmeasured[0].matches_measured_size());
    // The measured range is the documented one.
    assert_eq!(MEASURED_MIN_EVENT_BYTES, 12);
    assert_eq!(MEASURED_MAX_EVENT_BYTES, 1156);
}

#[test]
fn accept_f40_d3_camera_sequence_declares_every_unmeasured_field_as_unknown() {
    let provenance = Provenance::designed(ClaimId::new("f40d3.test").unwrap());
    let track = ContentId::from_source(ContentKind::CameraTrack, "camera.c1.generic_intro")
        .expect("the track key is valid");
    let declared =
        declared_camera_sequence(Origin::SyntheticFixture, provenance, track.clone()).unwrap();

    // The measured pair: an in-engine presentation of this camera track.
    assert_eq!(
        declared.presentation(),
        &DeclaredPresentation::InEngine {
            camera_track: track.clone()
        }
    );
    // Every load-bearing value stays an explicit f40d3 unknown.
    assert!(!declared.duration_ticks().is_known());
    assert!(!declared.frame_size().is_known());
    assert!(!declared.skippable().is_known());
    assert!(!declared.pause().is_known());
    assert!(!declared.clock().is_known());
    assert!(!declared.recovery().is_known());
    assert!(declared.actions().is_empty());

    // Lowering refuses the record by claim: no invented number ever reaches
    // the runtime. The first unknown `lower_cinematic` meets on the in-engine
    // path is the pause policy.
    match lower_cinematic(&declared).unwrap_err() {
        CinematicLowerError::Unknown {
            field, claim_id, ..
        } => {
            assert_eq!(field, "pause");
            assert_eq!(claim_id.as_str(), "f40d3.pause_policy");
        }
        other => panic!("expected an unknown-field refusal, got {other:?}"),
    }
}

/// The eight retail world groups that each ship a camera carrier, measured.
const RETAIL_GROUPS: [&str; 8] = ["C1", "C1B", "C1C", "C2", "C2B", "C3", "C4", "C5"];

/// The camera-cinematic names measured in **every** retail camera carrier.
const RETAIL_COMMON_SEQUENCES: [&str; 3] = ["generic_intro", "gi_scene1", "player_setup"];

fn game_dir() -> std::path::PathBuf {
    std::env::var("CS_GAME_DIR")
        .expect("CS_GAME_DIR must name the original installation for this test")
        .into()
}

#[test]
#[ignore = "requires CS_GAME_DIR: the original installation is needed"]
fn accept_f40_d3_retail_camera_carriers_walk_their_camera_cinematics() {
    for group in RETAIL_GROUPS {
        let spelling = format!("ZBD/{group}/cam_anim.zbd");
        let path = game_dir().join(&spelling);
        let bytes = std::fs::read(&path)
            .unwrap_or_else(|error| panic!("{spelling}: cannot read the carrier: {error}"));
        let relative =
            RelativePath::new(&spelling).expect("the on-disk spelling is a relative path");
        let survey = cs_app::cinematics::camera_anim::survey_camera_carrier(&relative, &bytes)
            .unwrap_or_else(|error| panic!("{spelling}: survey refused: {error}"));

        assert_eq!(survey.version(), 53, "{spelling}: the measured version");
        assert!(
            survey.total_records() > 0,
            "{spelling}: records were walked"
        );
        assert!(
            survey.streams_parsed() > 0 && survey.events_parsed() > 0,
            "{spelling}: the camera cinematics' streams were walked"
        );
        // The camera cinematics are the camera1-rooted records, and the
        // measured common set is present in every carrier.
        let names: Vec<&[u8]> = survey
            .sequences()
            .iter()
            .map(|sequence| sequence.anim_name.as_slice())
            .collect();
        for expected in RETAIL_COMMON_SEQUENCES {
            assert!(
                names.contains(&expected.as_bytes()),
                "{spelling}: the camera cinematic {expected} is missing (found {names:?})"
            );
        }
        // Every record is classified by the measured rule, every block parsed,
        // and every event matches its type's measured size — no unmeasured
        // type and no size mismatch on retail bytes.
        for sequence in survey.sequences() {
            assert_eq!(
                sequence.object_name, CAMERA_OBJECT_NAME,
                "{spelling}: a surveyed record is not camera1-rooted"
            );
            assert_eq!(sequence.root_name, CAMERA_OBJECT_NAME);
            assert!(
                !sequence.blocks.is_empty(),
                "{spelling}: record {} has no sequence block",
                String::from_utf8_lossy(&sequence.anim_name)
            );
            for block in &sequence.blocks {
                for event in &block.events {
                    assert!(
                        event.matches_measured_size(),
                        "{spelling}: event 0x{:x} stored {} bytes, which its type's measured \
                         size does not contain",
                        event.type_id(),
                        event.total_bytes()
                    );
                }
            }
        }
        assert!(
            survey.unmeasured_types().is_empty(),
            "{spelling}: unmeasured event types on retail: {:?}",
            survey.unmeasured_types()
        );
        assert!(
            survey.size_mismatches().is_empty(),
            "{spelling}: size mismatches on retail: {:?}",
            survey.size_mismatches()
        );
    }
}

/// The camera cinematics measured in **every** retail camera carrier: the
/// three common records, plus the `speed_cue`(`2`) variants that differ per
/// carrier.
fn expected_world_camera_names(group: &str) -> Vec<String> {
    let mut names = vec![
        "generic_intro".to_owned(),
        "gi_scene1".to_owned(),
        "player_setup".to_owned(),
    ];
    match group {
        "C1B" => names.push("speed_cue2".to_owned()),
        "C2B" => {
            names.push("speed_cue".to_owned());
            names.push("speed_cue2".to_owned());
        }
        _ => names.push("speed_cue".to_owned()),
    }
    names
}

/// The exact camera-cinematic inventory of the retail installation, measured
/// by a full-carrier scan on 2026-10-13 (review of the first draft corrected
/// its `c4/m01` mis-attribution): carrier key (`ZBD/<GROUP>` for a camera
/// carrier, `ZBD/<GROUP>/<MISSION>` for a mission carrier) to the record
/// names, in carrier order.
fn expected_camera_inventory() -> BTreeMap<String, Vec<String>> {
    let mut expected = BTreeMap::new();
    for group in ["C1", "C1B", "C1C", "C2", "C2B", "C3", "C4", "C5"] {
        expected.insert(format!("ZBD/{group}"), expected_world_camera_names(group));
    }
    expected.insert(
        "ZBD/C1/M04".to_owned(),
        vec!["mission_intro_animation".to_owned(), "scene1".to_owned()],
    );
    expected.insert(
        "ZBD/C4/M03".to_owned(),
        vec!["bdrop_ew_cam".to_owned(), "bdrop_we_cam".to_owned()],
    );
    // The 21 multiplayer mission carriers (groups without an MP2 or MP3
    // ship fewer); every one carries exactly `multiplayer_setup`.
    for group in ["C1", "C1B", "C1C", "C2", "C2B", "C3", "C4", "C5"] {
        let missions: &[&str] = match group {
            "C1" | "C2" | "C3" | "C4" | "C5" => &["MP1", "MP2", "MP3"],
            _ => &["MP1", "MP3"],
        };
        for mission in missions {
            expected.insert(
                format!("ZBD/{group}/{mission}"),
                vec!["multiplayer_setup".to_owned()],
            );
        }
    }
    expected
}

/// The event type ids measured **inside** camera cinematics (all 58 records,
/// all 31 carriers). Notably absent: `0x114` — the one opcode the
/// installation itself names `CAMERA_STATE` (#690) occurs in no camera
/// cinematic, and `0x11a`/`0x11c` are #690's unjoined opcodes 26/28, which
/// `ZBD/C1/M04`'s `mission_intro_animation` reset block carries exactly once
/// each.
fn expected_camera_cinematic_types() -> BTreeSet<u32> {
    [
        0x101, 0x106, 0x107, 0x109, 0x10c, 0x10f, 0x110, 0x116, 0x118, 0x119, 0x11a, 0x11c, 0x11f,
        0x120, 0x121, 0x122, 0x123, 0x124, 0x12a, 0x21e, 0x319,
    ]
    .into_iter()
    .collect()
}

/// Walks **every** retail animation carrier (all camera and mission ones)
/// through the production survey and pins the full camera-cinematic
/// inventory: the exact per-carrier record names, the record and carrier
/// totals, the exact event type set inside camera cinematics, the two
/// unjoined-opcode events, and that `CAMERA_STATE` is not among them.
#[test]
#[ignore = "requires CS_GAME_DIR: the original installation is needed"]
fn accept_f40_d3_retail_mission_camera_cinematics_are_inventoried() {
    let root = game_dir();

    // Enumerate every carrier the way the census does: ZBD/<group>/cam_anim.zbd
    // and ZBD/<group>/<mission>/mis_anim.zbd.
    let mut carriers: Vec<(String, String, std::path::PathBuf)> = Vec::new();
    for group_entry in std::fs::read_dir(root.join("ZBD")).expect("the installation has ZBD") {
        let group_path = group_entry.expect("a ZBD entry").path();
        if !group_path.is_dir() {
            continue;
        }
        let group = group_path
            .file_name()
            .unwrap()
            .to_string_lossy()
            .into_owned();
        let cam = group_path.join("cam_anim.zbd");
        if cam.is_file() {
            carriers.push((
                format!("ZBD/{group}"),
                format!("ZBD/{group}/cam_anim.zbd"),
                cam,
            ));
        }
        for mission_entry in std::fs::read_dir(&group_path).expect("a group directory") {
            let mission_path = mission_entry.expect("a group entry").path();
            if !mission_path.is_dir() {
                continue;
            }
            let mis = mission_path.join("mis_anim.zbd");
            if mis.is_file() {
                let mission = mission_path
                    .file_name()
                    .unwrap()
                    .to_string_lossy()
                    .into_owned();
                carriers.push((
                    format!("ZBD/{group}/{mission}"),
                    format!("ZBD/{group}/{mission}/mis_anim.zbd"),
                    mis,
                ));
            }
        }
    }
    carriers.sort_by(|a, b| a.0.cmp(&b.0));
    assert_eq!(
        carriers.len(),
        61,
        "the measured carrier population (8 camera + 53 mission)"
    );

    let mut inventory: BTreeMap<String, Vec<String>> = BTreeMap::new();
    let mut camera_records = 0usize;
    let mut camera_events = 0u64;
    let mut type_counts: BTreeMap<u32, u64> = BTreeMap::new();
    for (key, spelling, path) in &carriers {
        let bytes = std::fs::read(path)
            .unwrap_or_else(|error| panic!("{spelling}: cannot read the carrier: {error}"));
        let relative =
            RelativePath::new(spelling).expect("the on-disk spelling is a relative path");
        let survey = cs_app::cinematics::camera_anim::survey_camera_carrier(&relative, &bytes)
            .unwrap_or_else(|error| panic!("{spelling}: survey refused: {error}"));
        assert_eq!(survey.version(), 53, "{spelling}: the measured version");
        if survey.sequences().is_empty() {
            continue;
        }
        let names: Vec<String> = survey
            .sequences()
            .iter()
            .map(|sequence| String::from_utf8_lossy(&sequence.anim_name).into_owned())
            .collect();
        camera_records += names.len();
        for sequence in survey.sequences() {
            assert_eq!(sequence.object_name, CAMERA_OBJECT_NAME);
            assert_eq!(sequence.root_name, CAMERA_OBJECT_NAME);
            for block in &sequence.blocks {
                for event in &block.events {
                    camera_events += 1;
                    *type_counts.entry(event.type_id()).or_insert(0) += 1;
                }
            }
        }
        inventory.insert(key.clone(), names);
    }

    assert_eq!(
        inventory,
        expected_camera_inventory(),
        "the exact per-carrier camera-cinematic inventory"
    );
    assert_eq!(camera_records, 58, "the measured camera-cinematic records");
    let camera_types: BTreeSet<u32> = type_counts.keys().copied().collect();
    assert_eq!(
        camera_types,
        expected_camera_cinematic_types(),
        "the exact event type set inside camera cinematics"
    );
    assert_eq!(
        camera_events, 1076,
        "the measured event total inside camera cinematics"
    );
    // The installation's own camera-named opcode is not the pose source: it
    // occurs in no camera cinematic (#690: its 10 events live in
    // gi_1stperson/1stperson/cabpkup_player records).
    assert!(
        !camera_types.contains(&0x114),
        "CAMERA_STATE (0x114) occurs inside a camera cinematic"
    );
    // The mission-intro cinematic's two events of #690's unjoined opcodes,
    // exactly once each.
    assert_eq!(type_counts.get(&0x11a), Some(&1), "one opcode-26 event");
    assert_eq!(type_counts.get(&0x11c), Some(&1), "one opcode-28 event");
}
