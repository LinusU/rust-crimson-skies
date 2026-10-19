//! PARITY-M01-ATMO-E5 (#1685): what writes the M01 player body before launch
//! callback 10, and what ends the cinematic state (finding
//! `docs/findings/2026-10-19-m01-launch-body-pose.md`).
//!
//! The synthetic members run in CI. The retail members read `$CS_GAME_DIR`
//! (never write it) through production code and fail loudly without it; one
//! reads `$CS_ENGINE_IMAGE` for the instruction bytes the finding rests on.

use std::path::PathBuf;

use cs_app::cinematics::launch_body::{
    BODY_CALLBACK_IDS, BODY_ENGINE_FACTS, BODY_UNKNOWNS, BlockKind, BodyEventKind, INTRO_RAISE,
    INTRO_SPEED_FACTOR, INTRO_THROTTLE, measure_intro_body_events, name_occurrences,
    place_player_at_intro_start,
};
use cs_app::cinematics::launch_end_pose::LAUNCH_INTRO_ANIMATION;
use cs_app::mission_start::recover_retail_aircraft_records;
use cs_types::install::RelativePath;

const CAM_ANIM: &str = "ZBD/C3/cam_anim.zbd";
const MIS_ANIM: &str = "ZBD/C3/M01/mis_anim.zbd";
const ZRDR: &str = "ZBD/C3/M01/zrdr.zbd";

fn root() -> PathBuf {
    PathBuf::from(std::env::var("CS_GAME_DIR").expect("a retail test needs CS_GAME_DIR to be set"))
}

fn read(relative: &str) -> (RelativePath, Vec<u8>) {
    let bytes = std::fs::read(root().join(relative)).expect("the carrier is readable");
    (
        RelativePath::new(&relative.to_ascii_lowercase()).expect("a relative path"),
        bytes,
    )
}

fn close(got: f32, want: f32) -> bool {
    (got - want).abs() < 1e-4
}

#[test]
fn accept_parity_m01_atmo_e5_synthetic_placement_follows_the_engine_arithmetic() {
    // Heading 0: the (0,0,1) axis is unchanged, so the velocity is (0,0,-20).
    let straight = place_player_at_intro_start([10.0, 150.0, -20.0], 0.0);
    assert_eq!(straight.position, [10.0, 150.0 + INTRO_RAISE, -20.0]);
    assert_eq!(straight.euler, [0.0, 0.0, 0.0]);
    assert!((straight.throttle - INTRO_THROTTLE).abs() < f32::EPSILON);
    assert!(close(straight.velocity[0], 0.0));
    assert!(close(straight.velocity[1], 0.0));
    assert!(close(straight.velocity[2], INTRO_SPEED_FACTOR));

    // Heading 90 degrees turns +Z toward +X: the velocity points at -X.
    let east = place_player_at_intro_start([0.0; 3], 90.0);
    assert!(close(east.velocity[0], INTRO_SPEED_FACTOR));
    assert!(close(east.velocity[2], 0.0));
    assert!(close(east.euler[1], std::f32::consts::FRAC_PI_2));

    // The speed is the factor whatever the heading, and y never moves.
    for heading in [-170.0, -33.0, 40.0, 123.0, 359.0] {
        let body = place_player_at_intro_start([1.0, 2.0, 3.0], heading);
        let speed = body.velocity.iter().map(|v| v * v).sum::<f32>().sqrt();
        assert!(close(speed, INTRO_SPEED_FACTOR.abs()), "{heading}: {speed}");
        assert!(close(body.velocity[1], 0.0));
        assert!(close(body.position[1], 2.0 + INTRO_RAISE));
    }
}

#[test]
fn accept_parity_m01_atmo_e5_synthetic_refusals_and_named_unknowns() {
    let path = RelativePath::new("zbd/c3/cam_anim.zbd").expect("a relative path");
    assert!(measure_intro_body_events(&path, &[0u8; 64], LAUNCH_INTRO_ANIMATION).is_err());
    assert_eq!(name_occurrences(b"abc texdrop texdrop", "texdrop"), 2);
    assert_eq!(name_occurrences(b"abc", ""), 0);

    let ids: Vec<&str> = BODY_UNKNOWNS.iter().map(|u| u.id).collect();
    for required in [
        "node-detach-after-intro",
        "cinematic-flag-clear",
        "airframe-mode",
        "network-gate-value",
        "no-run",
    ] {
        assert!(
            ids.contains(&required),
            "the unknown `{required}` is missing"
        );
    }
    for unknown in BODY_UNKNOWNS {
        assert!(!unknown.what.is_empty() && !unknown.affects.is_empty());
        assert!(
            !unknown.resolver.is_empty(),
            "{} has no resolver",
            unknown.id
        );
    }
    for fact in BODY_ENGINE_FACTS {
        assert!(
            fact.va >= 0x40_1000 && !fact.claim.is_empty(),
            "{}",
            fact.id
        );
    }
    assert!(BODY_CALLBACK_IDS.contains(&10) && BODY_CALLBACK_IDS.contains(&951));
}

#[test]
#[ignore = "requires CS_GAME_DIR"]
fn accept_parity_m01_atmo_e5_retail_chain_moves_the_player_node_only_at_time_zero() {
    let (path, bytes) = read(CAM_ANIM);
    let events = measure_intro_body_events(&path, &bytes, LAUNCH_INTRO_ANIMATION)
        .expect("the intro chain reads");

    // Every parent change of the player node in the chain, in order.
    let moves: Vec<_> = events
        .iter()
        .filter_map(|e| match &e.kind {
            BodyEventKind::AddChild(parent) => Some((e.block, e.sequence.as_str(), "add", parent)),
            BodyEventKind::DeleteChild(parent) => {
                Some((e.block, e.sequence.as_str(), "delete", parent))
            }
            _ => None,
        })
        .collect();
    let expected = [
        (BlockKind::Reset, "RESET_SEQUENCE", "delete", "piratezep"),
        (BlockKind::Reset, "RESET_SEQUENCE", "add", "world1"),
        (BlockKind::Sequence, "start_script", "delete", "world1"),
        (BlockKind::Sequence, "start_script", "add", "piratezep"),
    ];
    assert_eq!(moves.len(), expected.len(), "{moves:?}");
    for (got, want) in moves.iter().zip(expected) {
        assert_eq!((got.0, got.1, got.2, got.3.as_str()), want);
    }
    // All four are at START_TIME 0: nothing in the chain detaches the player
    // after the intro starts.
    assert!(
        events
            .iter()
            .filter(|e| matches!(
                e.kind,
                BodyEventKind::AddChild(_) | BodyEventKind::DeleteChild(_)
            ))
            .all(|e| e.record == LAUNCH_INTRO_ANIMATION && e.start_time == 0.0)
    );

    // The body callbacks the chain fires: 10 in the reset block between the
    // delete and the add, 20 and 11 in `callback_sequence`; never 0, 12 or 951.
    let callbacks: Vec<(u32, BlockKind, &str)> = events
        .iter()
        .filter_map(|e| match e.kind {
            BodyEventKind::Callback(id) => Some((id, e.block, e.sequence.as_str())),
            _ => None,
        })
        .collect();
    assert_eq!(
        callbacks,
        [
            (10, BlockKind::Reset, "RESET_SEQUENCE"),
            (20, BlockKind::Sequence, "callback_sequence"),
            (11, BlockKind::Sequence, "callback_sequence"),
        ]
    );
    let reset: Vec<_> = events
        .iter()
        .filter(|e| e.block == BlockKind::Reset)
        .map(|e| &e.kind)
        .collect();
    assert!(matches!(reset[0], BodyEventKind::DeleteChild(_)));
    assert_eq!(reset[1], &BodyEventKind::Callback(10));
    assert!(matches!(reset[2], BodyEventKind::AddChild(_)));
}

#[test]
#[ignore = "requires CS_GAME_DIR"]
fn accept_parity_m01_atmo_e5_retail_no_data_names_the_release_animations() {
    // The only callback-951 sites are `texdrop` and `pzep_launch_player`
    // (#1678). `texdrop` is called by the mission program (the dropoff
    // approach); `pzep_launch_player` is named once, by its own record, in the
    // whole of the three carriers.
    let (_, mis) = read(MIS_ANIM);
    let (_, cam) = read(CAM_ANIM);
    let (_, zrdr) = read(ZRDR);
    assert_eq!(name_occurrences(&mis, "pzep_launch_player"), 1);
    assert_eq!(name_occurrences(&cam, "pzep_launch_player"), 0);
    assert_eq!(name_occurrences(&zrdr, "pzep_launch_player"), 0);
    assert!(name_occurrences(&zrdr, "texdrop") >= 6);
}

#[test]
#[ignore = "requires CS_GAME_DIR"]
fn accept_parity_m01_atmo_e5_retail_player_is_the_first_record_and_the_placed_pose_follows() {
    let aircraft = recover_retail_aircraft_records(&root(), "zbd/c3/m01").expect("aiv.zrd reads");
    let first = aircraft
        .iter()
        .find(|a| a.enabled == Some(true))
        .expect("an enabled record");
    assert_eq!(
        first.name, "player",
        "the first spawned record is the player"
    );
    let stored = first.stored_pose.as_ref().expect("a stored pose");
    assert_eq!(stored.position_metres(), [-1426.0, 150.0, -1813.0]);
    assert_eq!(stored.heading, 40.0);

    let body = place_player_at_intro_start(stored.position_metres(), stored.heading);
    assert_eq!(body.position, [-1426.0, 250.0, -1813.0]);
    assert!(close(body.euler[1], 40.0_f32.to_radians()));
    assert!(close(body.velocity[0], -20.0 * 40.0_f32.to_radians().sin()));
    assert!(close(body.velocity[2], -20.0 * 40.0_f32.to_radians().cos()));
    assert!(close(body.velocity[1], 0.0));
}

#[test]
#[ignore = "requires CS_ENGINE_IMAGE"]
fn accept_parity_m01_atmo_e5_engine_facts_are_the_bytes_the_finding_read() {
    let image = cs_content::coordinates::load_engine_image().expect("CS_ENGINE_IMAGE names it");
    assert_eq!(
        image.digest,
        cs_content::coordinates::original_image_digest()
    );
    let bytes = image.bytes.as_slice();
    let at = |va: usize| va - 0x40_0000;
    let dword = |va: usize| u32::from_le_bytes(bytes[at(va)..at(va) + 4].try_into().unwrap());
    let float = |va: usize| f32::from_bits(dword(va));
    let has = |va: usize, want: &[u8]| &bytes[at(va)..at(va) + want.len()] == want;
    let call_target = |va: usize| -> usize {
        assert_eq!(bytes[at(va)], 0xe8, "a call at {va:#x}");
        (va + 5).wrapping_add(dword(va + 1) as i32 as usize)
    };
    let callers = |target: usize| -> Vec<usize> {
        (0x40_1000..0x60_1000 - 5)
            .filter(|&va| bytes[at(va)] == 0xe8 && call_target(va) == target)
            .collect()
    };
    let text_at = |va: usize| {
        let tail = &bytes[at(va)..];
        let end = tail.iter().position(|&b| b == 0).unwrap();
        std::str::from_utf8(&tail[..end]).unwrap().to_owned()
    };
    for fact in BODY_ENGINE_FACTS {
        assert!((fact.va as usize) < 0x60_1000, "{}", fact.id);
    }

    // player-vehicle-pointer: `mov eax,[esp+4]; mov [0x71c298],eax; ret`.
    assert!(has(
        0x473060,
        &[0x8b, 0x44, 0x24, 0x04, 0xa3, 0x98, 0xc2, 0x71, 0, 0xc3]
    ));
    assert_eq!(callers(0x473060), vec![0x475397, 0x47bc35, 0x48003c]);

    // vehicle-constructor-zeroes-pose: the loader calls 0x4aff80, whose body
    // stores the zero vector at +0x1f8 / +0x204 and zero at +0x150.
    assert_eq!(call_target(0x47c51d), 0x4aff80);
    assert!(has(0x4b0757, &[0x8d, 0x8e, 0xf8, 0x01, 0, 0]));
    assert!(has(0x4b0776, &[0x8d, 0x86, 0x04, 0x02, 0, 0]));
    assert!(has(0x4b0696, &[0x89, 0x9e, 0x50, 0x01, 0, 0]));
    // The zero vector at 0x75d1b8 lies past the file's raw data (`.bss`).
    assert!(at(0x75d1b8) >= bytes.len());

    // loader-places-the-node: position from record+0x18, heading from
    // record+0x24 times the double pi/180, set on the node before the vehicle.
    assert!(has(0x47c2a6, &[0x8d, 0x4f, 0x18]));
    assert_eq!(call_target(0x47c4c0), 0x4d1d50);
    assert!(has(0x47c4e8, &[0xd9, 0x47, 0x24]));
    assert!(has(0x47c4ee, &[0xdc, 0x0d, 0xe8, 0x40, 0x60, 0]));
    let degrees_to_radians = f64::from_bits(u64::from_le_bytes(
        bytes[at(0x6040e8)..at(0x6040e8) + 8].try_into().unwrap(),
    ));
    assert!((degrees_to_radians - std::f64::consts::PI / 180.0).abs() < 1e-12);
    assert_eq!(call_target(0x47c4fa), 0x4d1a30);

    // loader-copies-node-to-body: 0x476250 after the constructor.
    assert_eq!(call_target(0x47c77b), 0x476250);
    assert_eq!(callers(0x476250), vec![0x47c77b]);
    assert_eq!(call_target(0x476282), 0x4cf200);
    assert!(has(0x47628d, &[0x8d, 0xbe, 0x04, 0x02, 0, 0]));
    assert_eq!(call_target(0x4762b6), 0x4d1b40);
    assert_eq!(call_target(0x476301), 0x53f610);
    assert!(has(0x47636b, &[0xc6, 0x86, 0x1d, 0x09, 0, 0, 0x01]));
    assert!(has(0x476375, &[0xc6, 0x86, 0x1e, 0x09, 0, 0, 0x00]));

    // mission-load-clears-then-sets-the-flags.
    assert_eq!(call_target(0x4655b3), 0x4735b0);
    assert_eq!(call_target(0x475397), 0x473060);
    assert!(has(0x4753a1, &[0xc6, 0x80, 0x1d, 0x09, 0, 0, 0x00]));
    assert!(has(0x46563f, &[0xc6, 0x80, 0x1d, 0x09, 0, 0, 0x01]));
    assert!(has(0x465646, &[0xc6, 0x80, 0x1e, 0x09, 0, 0, 0x00]));
    assert!(has(0x46564d, &[0x68, 0x50, 0x5e, 0x62, 0]));
    assert!(has(0x465652, &[0x68, 0x60, 0x5e, 0x62, 0]));
    assert_eq!(call_target(0x465659), 0x46c370);
    assert_eq!(text_at(0x625e50), "NEW_GAME_START");
    assert_eq!(text_at(0x625e60), "StartAnims.zrd");

    // pilot-record-is-not-the-body: the keys beside the +0x204 stores.
    assert!(has(0x47b1fe, &[0x89, 0x87, 0x04, 0x02, 0, 0]));
    assert!(has(0x47b2e8, &[0x89, 0x8f, 0x04, 0x02, 0, 0]));
    assert_eq!(text_at(0x628104), "dare_devil");
    assert_eq!(text_at(0x628110), "natural_touch");
    assert_eq!(text_at(0x628120), "sixth_sense");
    assert_eq!(text_at(0x628150), "stun_recovery");
    assert_eq!(text_at(0x628160), "preferred_engagement_altitude");

    // airframe-mode: the keys that set type+0xa4, and the loader's test.
    for (va, name) in [
        (0x628084, "mode"),
        (0x62808c, "tank"),
        (0x628094, "ship"),
        (0x62809c, "jet"),
        (0x6280a0, "heli"),
        (0x6280a8, "wingman"),
        (0x6280b0, "plane"),
    ] {
        assert_eq!(text_at(va), name);
    }
    assert!(has(0x47aff1, &[0xc7, 0x87, 0xa4, 0, 0, 0, 0x02, 0, 0, 0]));
    assert!(has(0x47b00f, &[0xc7, 0x87, 0xa4, 0, 0, 0, 0x03, 0, 0, 0]));
    assert!(has(0x47c2b4, &[0x8b, 0x83, 0xa4, 0, 0, 0]));
    assert_eq!(call_target(0x47c2e5), 0x47fad0);

    // place-routine-intro-branch.
    assert_eq!(call_target(0x47e15f), 0x47f1f0);
    assert!(has(0x47f25f, &[0x38, 0x98, 0x1d, 0x09, 0, 0]));
    assert!(has(0x47f26b, &[0xd9, 0x80, 0x08, 0x02, 0, 0]));
    assert!(has(0x47f274, &[0xd8, 0x05, 0xe4, 0x32, 0x60, 0]));
    assert_eq!(float(0x6032e4), INTRO_RAISE);
    assert!(has(
        0x47f28b,
        &[0xc7, 0x80, 0x24, 0x01, 0, 0, 0xcd, 0xcc, 0xcc, 0x3e]
    ));
    assert_eq!(float(0x47f28b + 6), INTRO_THROTTLE);
    assert_eq!(call_target(0x47f2a2), 0x53fb40);
    assert!(has(0x47f27b, &[0x68, 0xd0, 0x79, 0x63, 0]));
    assert_eq!(
        [float(0x6379d0), float(0x6379d4), float(0x6379d8)],
        [0.0, 0.0, 1.0]
    );
    assert!(has(0x47f2aa, &[0xd8, 0x0d, 0x40, 0x80, 0x60, 0]));
    assert_eq!(float(0x608040), INTRO_SPEED_FACTOR);
    assert_eq!(float(0x6032e0), 0.5);
    // The euler-to-quaternion call passes (yaw=+0x1fc, +0x1f8, +0x200).
    assert_eq!(call_target(0x47f483), 0x53f610);

    // place-routine-writes-node-and-clears-flags.
    assert_eq!(call_target(0x47f5ea), 0x4d1d50);
    assert_eq!(call_target(0x47f60d), 0x4d1a30);
    assert!(has(0x47f625, &[0x88, 0x98, 0x1d, 0x09, 0, 0]));
    assert!(has(0x47f62b, &[0x88, 0x98, 0x1f, 0x09, 0, 0]));
    assert!(has(0x47f631, &[0x88, 0x98, 0x1e, 0x09, 0, 0]));

    // The handlers of ids 0, 11 and 12 in the dispatch table.
    assert_eq!(dword(0x47f160), 0x47e0ab);
    assert_eq!(dword(0x47f160 + 4 * 11), 0x47e19e);
    assert_eq!(dword(0x47f160 + 4 * 12), 0x47e1ed);
    assert!(has(0x47e1ae, &[0xc6, 0x80, 0x1d, 0x09, 0, 0, 0x01]));
    assert!(has(0x47e1b5, &[0xc6, 0x80, 0x1e, 0x09, 0, 0, 0x00]));
    assert!(has(0x47e1c4, &[0xc6, 0x81, 0x1e, 0x09, 0, 0, 0x01]));
    assert_eq!(call_target(0x47e294), 0x4edda0);
    assert!(has(
        0x47e0bc,
        &[0xc7, 0x05, 0x0c, 0xc5, 0x71, 0, 0, 0, 0, 0]
    ));

    // release-handler-copies-node-to-body.
    assert_eq!(call_target(0x47e565), 0x4cf200);
    assert_eq!(call_target(0x47e578), 0x4cf380);
    assert_eq!(call_target(0x47e69d), 0x4cd6d0);
    assert_eq!(call_target(0x47e6b7), 0x4d1d50);
    assert_eq!(call_target(0x47e6d1), 0x4d1a30);

    // animation-tick-steps-the-reset-block and skip-key-path.
    assert!(has(0x4ed2c7, &[0x8b, 0xb7, 0xd0, 0, 0, 0]));
    assert_eq!(call_target(0x4ed2f5), 0x4ecbb0);
    assert_eq!(call_target(0x4ed332), 0x4ed190);
    assert!(has(0x4a02ac, &[0x39, 0x1d, 0x0c, 0xc5, 0x71, 0]));
    assert_eq!(call_target(0x4a02b5), 0x536000);
    assert!(has(0x4a02c8, &[0x89, 0x1d, 0x0c, 0xc5, 0x71, 0]));
    assert_eq!(call_target(0x4a02ce), 0x4ed480);

    // The debug start pose `[0x71daca]` has one setter and one clear, so the
    // `0x47f1f0` branch on it is not taken in an ordinary start.
    let flag = [0xca, 0xda, 0x71, 0x00];
    let stores: Vec<usize> = (0x40_1000..0x60_1000 - 7)
        .filter(|&va| {
            (bytes[at(va)] == 0xc6
                && bytes[at(va + 1)] == 0x05
                && bytes[at(va + 2)..at(va + 6)] == flag)
                || (bytes[at(va)] == 0xa2 && bytes[at(va + 1)..at(va + 5)] == flag)
        })
        .collect();
    assert_eq!(stores, vec![0x4a75fe, 0x4b376e]);

    // network-gate-value: the pointer has one writer, which looks up the
    // config variable `Network`; `0x440ad0` reads through it.
    assert!(has(0x440247, &[0xa3, 0x50, 0xf7, 0x64, 0]));
    assert_eq!(text_at(0x6237f4), "Network");
    assert!(has(
        0x440ad0,
        &[0xa1, 0x50, 0xf7, 0x64, 0, 0x8b, 0x00, 0xc3]
    ));
}
