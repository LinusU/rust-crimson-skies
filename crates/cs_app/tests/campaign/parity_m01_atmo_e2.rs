//! PARITY-M01-ATMO-E2 (#1660): the player's pose at the end of the M01 launch
//! cinematic (finding `docs/findings/2026-10-18-m01-launch-end-pose.md`).
//!
//! The synthetic members run in CI. The retail members read `$CS_GAME_DIR`
//! (never write it) through production code, recompute what they compare
//! against from the members' own bytes and fail loudly without it; one reads
//! `$CS_ENGINE_IMAGE` for the instruction bytes the finding's re-parent claim
//! rests on.

use std::path::PathBuf;

use cs_app::cinematics::launch_end_pose::{
    DROP_SEQUENCE, LAUNCH_END_POSE_UNKNOWNS, LAUNCH_INTRO_ANIMATION, LaunchEndPoseError,
    ParentPose, measure_launch_end_pose,
};
use cs_app::mission_start::recover_retail_aircraft_records;
use cs_app::mission_world_actors::bind_mission_world_actors;
use cs_assets::install;
use cs_formats::script_raw::discovery::discover_container;
use cs_formats::zbd::zeppelins::{ZEPPELINS_MEMBER, read_zeppelins_member};
use cs_types::content::{ContentId, ContentKind};
use cs_types::install::RelativePath;

const CAM_ANIM: &str = "ZBD/C3/cam_anim.zbd";
const ARCHIVE: &str = "zbd/c3/m01/zrdr.zbd";

fn root() -> PathBuf {
    PathBuf::from(std::env::var("CS_GAME_DIR").expect("a retail test needs CS_GAME_DIR to be set"))
}

#[test]
fn accept_parity_m01_atmo_e2_synthetic_garbage_is_a_carrier_refusal_and_poses_compose() {
    let path = RelativePath::new("zbd/c3/cam_anim.zbd").expect("a relative path");
    let error = measure_launch_end_pose(&path, &[0u8; 64], LAUNCH_INTRO_ANIMATION, &[])
        .expect_err("bytes that are no animation container measure nothing");
    assert!(
        matches!(error, LaunchEndPoseError::Carrier(_)),
        "unexpected refusal {error}"
    );

    // Yaw 90° turns local +Z toward +X (`stored_heading_radians`): a unit
    // quaternion about Y with half-angle 45°.
    let pose = ParentPose::from_stored("zep", [1.0, 2.0, 3.0], 90.0, 0.0);
    let [x, y, z, w] = pose.orientation;
    assert!(x.abs() < 1e-12 && z.abs() < 1e-12);
    assert!((y - std::f64::consts::FRAC_1_SQRT_2).abs() < 1e-7);
    assert!((w - std::f64::consts::FRAC_1_SQRT_2).abs() < 1e-7);
    assert_eq!(pose.position_m, [1.0, 2.0, 3.0]);
}

#[test]
fn accept_parity_m01_atmo_e2_unknowns_are_named_with_a_resolver() {
    let ids: Vec<&str> = LAUNCH_END_POSE_UNKNOWNS.iter().map(|u| u.id).collect();
    for required in [
        "rotation-convention",
        "parent-of-parent",
        "parent-motion",
        "intro-clock",
        "release-rule",
        "release-velocity",
    ] {
        assert!(
            ids.contains(&required),
            "the unknown `{required}` is missing"
        );
    }
    for unknown in LAUNCH_END_POSE_UNKNOWNS {
        assert!(!unknown.what.is_empty() && !unknown.affects.is_empty());
        assert!(
            !unknown.resolver.is_empty(),
            "{} has no resolver",
            unknown.id
        );
    }
}

#[test]
#[ignore = "requires CS_GAME_DIR"]
fn accept_parity_m01_atmo_e2_retail_end_pose_follows_the_stated_chain() {
    let root = root();
    let bytes = std::fs::read(root.join(CAM_ANIM)).expect("cam_anim.zbd is readable");
    let path = RelativePath::new("zbd/c3/cam_anim.zbd").expect("a relative path");

    // The Pandora's pose, as the production world-actor binding leaves it.
    let found = install::discover(&root).expect("the installation is discoverable");
    let subject = ContentId::from_source(ContentKind::Mission, "ch1-m01").expect("id");
    let bound = bind_mission_world_actors(&root, &found, "zbd/c3/m01", "zbd/c3", &subject, 64);
    let row = bound
        .rows()
        .iter()
        .find(|row| row.node == "piratezep")
        .expect("M01 places the Pandora");
    let parent = ParentPose::from_zeppelin(row).expect("its position and attitude are measured");

    // The same record, decoded independently of the binding.
    let record = {
        let record = found
            .manifest
            .files
            .iter()
            .find(|record| record.relative_spelling.logical_key() == ARCHIVE)
            .expect("M01's reader archive is installed");
        let archive =
            std::fs::read(root.join(record.relative_spelling.as_str())).expect("readable");
        let discovery = discover_container(ARCHIVE, &record.relative_spelling, &archive);
        let member = discovery
            .programs()
            .iter()
            .find(|p| {
                p.locator()
                    .member()
                    .is_some_and(|m| m.eq_ignore_ascii_case(ZEPPELINS_MEMBER))
            })
            .expect("zeppelins.zrd is in the archive");
        let decoded = read_zeppelins_member(member.bytes()).expect("decodes");
        decoded
            .records()
            .iter()
            .find(|r| r.node() == "piratezep")
            .map(|r| (r.position(), r.yaw(), r.pitch()))
            .expect("a piratezep record")
    };
    assert_eq!(record.0, [-1401.4, 500.0, -1412.5]);
    assert_eq!((record.1, record.2), (40.0, 0.0));
    assert_eq!(parent.position_m, record.0, "no startup placement moves it");

    let pose = measure_launch_end_pose(&path, &bytes, LAUNCH_INTRO_ANIMATION, &[parent])
        .expect("the chain is the measured shape");

    // The chain, link by link.
    assert_eq!(pose.parent_node, "piratezep");
    assert_eq!(pose.parent_event.record, "generic_intro");
    assert_eq!(pose.parent_event.sequence, "start_script");
    assert_eq!(pose.drop_animation, "gi_playerdrop");
    assert_eq!(
        pose.stop_time_s, 1.95,
        "STOP_ANIMATION START_TIME 0x3ff9999a"
    );
    assert_eq!(pose.motion_event.record, "gi_playerdrop");
    assert!(pose.local.track_path.ends_with("gi_player2.zan"));
    assert_eq!(pose.local.block_index, 10);
    assert_eq!(pose.local.segment_index, 11);
    assert!((pose.local.segment_start_s - 1.8333).abs() < 1e-3);
    assert!((pose.local.segment_end_s - 1.9667).abs() < 1e-3);
    assert_eq!(DROP_SEQUENCE, "drop_planes");

    // The pose in the Pandora's frame (independent evaluation of segment 11's
    // cubics at u = 1.95 − 1.8333 gives these; tolerance is f32 rounding).
    let near = |got: f32, want: f32, tol: f32, what: &str| {
        assert!((got - want).abs() <= tol, "{what}: {got} vs {want}");
    };
    near(pose.local.position_m[0], 0.5918, 1e-3, "local x");
    near(pose.local.position_m[1], -63.2936, 2e-3, "local y");
    near(pose.local.position_m[2], 2.6339, 1e-3, "local z");
    near(pose.local.cubic_velocity_m_s[1], -20.02, 0.05, "cubic vy");
    near(
        pose.local.preceding_rate_m_s[1],
        -47.9,
        0.5,
        "preceding rate vy",
    );
    assert!(
        pose.local.preceding_rate_m_s[1] < 2.0 * pose.local.cubic_velocity_m_s[1],
        "the cubic eases to rest: its derivative is far below the rate it came in at"
    );
    assert!(
        pose.rotation_is_pure_pitch,
        "the quaternion has no Y or Z part"
    );
    near(
        pose.local.rotation_angle_degrees,
        76.78,
        0.05,
        "rotation angle",
    );

    // Carried through the Pandora's yaw (40° about Y, right-handed): the
    // test applies the rotation itself rather than reading the module's.
    let (sin, cos) = 40.0_f64.to_radians().sin_cos();
    let local = pose.local.position_m.map(f64::from);
    let expect = [
        f64::from(record.0[0]) + local[0] * cos + local[2] * sin,
        f64::from(record.0[1]) + local[1],
        f64::from(record.0[2]) - local[0] * sin + local[2] * cos,
    ];
    for (axis, (got, want)) in pose.composed_position_m.iter().zip(expect).enumerate() {
        assert!(
            (got - want).abs() < 1e-3,
            "composed axis {axis}: {got} vs {want}"
        );
    }

    // What the stored aiv.zrd player record is relative to it.
    let aircraft = recover_retail_aircraft_records(&root, "zbd/c3/m01").expect("aiv.zrd reads");
    let player = aircraft
        .iter()
        .find(|a| a.name == "player")
        .expect("a player record");
    let stored = player.stored_pose.as_ref().expect("a stored pose");
    assert_eq!(stored.position_metres(), [-1426.0, 150.0, -1813.0]);
    assert_eq!(
        stored.heading, record.1,
        "same heading as the Pandora's yaw"
    );
    let below = pose.composed_position_m[1] - f64::from(stored.position_metres()[1]);
    assert!(
        (285.0..290.0).contains(&below),
        "the stored start is {below} m below the end of the drop"
    );
    let horizontal = (pose.composed_position_m[0] - f64::from(stored.position_metres()[0]))
        .hypot(pose.composed_position_m[2] - f64::from(stored.position_metres()[2]));
    assert!(
        (400.0..410.0).contains(&horizontal),
        "the stored start is {horizontal} m away horizontally"
    );

    // A parent the chain does not name is refused, not substituted.
    let other = ParentPose::from_stored("cargozep1", [0.0; 3], 0.0, 0.0);
    let error = measure_launch_end_pose(&path, &bytes, LAUNCH_INTRO_ANIMATION, &[other])
        .expect_err("the Pandora's pose is required");
    assert!(matches!(error, LaunchEndPoseError::NoParentPose(ref n) if n == "piratezep"));
    let error = measure_launch_end_pose(&path, &bytes, "player_setup", &[parent])
        .expect_err("player_setup never parents the player");
    assert!(matches!(error, LaunchEndPoseError::Chain(_)), "{error}");
}

#[test]
#[ignore = "requires CS_ENGINE_IMAGE"]
fn accept_parity_m01_atmo_e2_reparent_executors_are_the_ones_the_finding_read() {
    let image = cs_content::coordinates::load_engine_image().expect("CS_ENGINE_IMAGE names it");
    assert_eq!(
        image.digest,
        cs_content::coordinates::original_image_digest()
    );
    let bytes = image.bytes.as_slice();
    let at = |va: usize| va - 0x40_0000;
    let dword = |va: usize| u32::from_le_bytes(bytes[at(va)..at(va) + 4].try_into().unwrap());

    // OBJECT_ADD_CHILD (0x4eab20) → node-type dispatch 0x4cd500 → type 5 →
    // 0x4d1390 → the list append 0x4cd610; DELETE (0x4eab90) → 0x4cd6d0 →
    // 0x4d13f0 → the list removal 0x4cd850.
    let call_target = |va: usize| -> usize {
        assert_eq!(bytes[at(va)], 0xe8, "a call at {va:#x}");
        (va + 5).wrapping_add(dword(va + 1) as i32 as usize)
    };
    assert_eq!(call_target(0x4eab77), 0x4cd500);
    assert_eq!(call_target(0x4eabbd), 0x4cd6d0);
    // The type-5 slot (index 4) of both jump tables.
    assert_eq!(dword(0x4cd5e0 + 4 * 4), 0x4cd561);
    assert_eq!(dword(0x4cd7c8 + 4 * 4), 0x4cd72d);
    assert_eq!(call_target(0x4cd563), 0x4d1390);
    assert_eq!(call_target(0x4cd72f), 0x4d13f0);
    assert_eq!(call_target(0x4d13e3), 0x4cd610);
    assert_eq!(call_target(0x4d1443), 0x4cd850);
    // Both list routines open with their register saves and neither is a
    // transform setter: the Object3d position setter is 0x4d1d50 and the
    // rotation setter 0x4d1a30, and neither routine's body calls them.
    assert_eq!(&bytes[at(0x4cd610)..at(0x4cd610) + 2], &[0x53, 0x55]);
    assert_eq!(&bytes[at(0x4cd850)..at(0x4cd850) + 2], &[0x53, 0x55]);
    for (start, end) in [(0x4cd610, 0x4cd6c5), (0x4cd850, 0x4cd95f)] {
        for va in start..end - 4 {
            if bytes[at(va)] == 0xe8 {
                let target = call_target(va);
                assert!(
                    target != 0x4d1d50 && target != 0x4d1a30,
                    "{va:#x} calls a transform setter {target:#x}"
                );
            }
        }
    }
}
