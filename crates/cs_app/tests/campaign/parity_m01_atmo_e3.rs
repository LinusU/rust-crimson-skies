//! PARITY-M01-ATMO-E3 (#1678): what places the player's body when the M01
//! launch drop animation stops (finding
//! `docs/findings/2026-10-18-m01-launch-release.md`).
//!
//! The synthetic members run in CI. The retail members read `$CS_GAME_DIR`
//! (never write it) through production code and fail loudly without it; one
//! reads `$CS_ENGINE_IMAGE` for the instruction bytes the finding rests on.

use std::path::PathBuf;

use cs_app::cinematics::launch_end_pose::{LAUNCH_INTRO_ANIMATION, LaunchEndPoseError};
use cs_app::cinematics::launch_release::{
    AUTO_RESET_NODE_STATES, CALLBACK_MEANINGS, ENGINE_FACTS, PLACE_PLAYER_CALLBACK_ID,
    RELEASE_CALLBACK_ID, RELEASE_UNKNOWNS, ResetStep, callback_sites, measure_launch_release,
};
use cs_types::install::RelativePath;

const CAM_ANIM: &str = "ZBD/C3/cam_anim.zbd";
const MIS_ANIM: &str = "ZBD/C3/M01/mis_anim.zbd";

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

#[test]
fn accept_parity_m01_atmo_e3_synthetic_garbage_is_a_carrier_refusal() {
    let path = RelativePath::new("zbd/c3/cam_anim.zbd").expect("a relative path");
    let error = measure_launch_release(&path, &[0u8; 64], LAUNCH_INTRO_ANIMATION)
        .expect_err("bytes that are no animation container measure nothing");
    assert!(
        matches!(error, LaunchEndPoseError::Carrier(_)),
        "unexpected refusal {error}"
    );
    let error = callback_sites(&path, &[0u8; 64], RELEASE_CALLBACK_ID)
        .expect_err("nor do they hold callbacks");
    assert!(matches!(error, LaunchEndPoseError::Carrier(_)), "{error}");
}

#[test]
fn accept_parity_m01_atmo_e3_unknowns_and_facts_are_named() {
    let ids: Vec<&str> = RELEASE_UNKNOWNS.iter().map(|u| u.id).collect();
    for required in [
        "body-state-at-intro-start",
        "node-detach-after-intro",
        "cinematic-flag-clear",
        "multiplayer-gate",
        "velocity-direction",
        "no-run",
    ] {
        assert!(
            ids.contains(&required),
            "the unknown `{required}` is missing"
        );
    }
    for unknown in RELEASE_UNKNOWNS {
        assert!(!unknown.what.is_empty() && !unknown.affects.is_empty());
        assert!(
            !unknown.resolver.is_empty(),
            "{} has no resolver",
            unknown.id
        );
    }
    for fact in ENGINE_FACTS {
        assert!(
            fact.va >= 0x40_1000 && !fact.claim.is_empty(),
            "{}",
            fact.id
        );
    }
    for id in [
        0,
        1,
        2,
        10,
        11,
        20,
        0x29b,
        0x391,
        0x392,
        RELEASE_CALLBACK_ID,
    ] {
        assert!(
            CALLBACK_MEANINGS.iter().any(|m| m.id == id),
            "callback {id} has no recorded meaning"
        );
    }
}

#[test]
#[ignore = "requires CS_GAME_DIR"]
fn accept_parity_m01_atmo_e3_retail_chain_never_restores_or_releases() {
    let (path, bytes) = read(CAM_ANIM);
    let release = measure_launch_release(&path, &bytes, LAUNCH_INTRO_ANIMATION)
        .expect("the chain is the measured shape");

    assert_eq!(release.intro, "generic_intro");
    assert_eq!(release.drop_animation, "gi_playerdrop");
    // The intro is an ON_CALL record (activation 3, started by name from
    // startanims.zrd) whose flag word has neither the
    // node-restore bit nor anything else of 0xc00010 but the two high bits.
    let intro = &release.chain[0];
    assert_eq!(intro.name, "generic_intro");
    assert_eq!(intro.flags, 0x00c0_0010);
    assert_eq!(intro.activation, 3);
    assert_eq!(intro.reset_time_s, 0.0);

    // The chain reaches the scenes, the first-person shot and the drop.
    let names: Vec<&str> = release.chain.iter().map(|a| a.name.as_str()).collect();
    for expected in [
        "gi_scene1",
        "gi_scene2",
        "gi_1stperson",
        "gi_playerdrop",
        "gi_pfighter2",
    ] {
        assert!(names.contains(&expected), "{expected} is not in {names:?}");
    }

    // Records that do carry AUTO_RESET_NODE_STATES exist (propellers, lights,
    // the letterbox, the speed cue) but none lists the player, the camera or
    // the Pandora among its objects, and every record that moves the player
    // lacks the bit. The drop's stop finishes at once.
    assert!(release.any_auto_reset_node_states);
    assert!(!release.auto_reset_restores_player);
    for animation in &release.chain {
        let restores = animation.flags & AUTO_RESET_NODE_STATES != 0;
        assert_eq!(restores, animation.auto_reset_node_states);
        if restores {
            for node in ["player", "camera1", "piratezep"] {
                assert!(
                    !animation.objects.iter().any(|name| name == node),
                    "{} restores `{node}`",
                    animation.name
                );
            }
        }
        if animation.objects.iter().any(|name| name == "player") {
            assert!(!restores, "{} would restore the player", animation.name);
        }
    }
    assert!(release.drop_stop_is_immediate);
    let drop = release
        .chain
        .iter()
        .find(|a| a.name == "gi_playerdrop")
        .expect("the drop is in the chain");
    assert_eq!(drop.reset_time_s, -1.0);

    // The chain never fires the release callback; it places the body once,
    // at the start, from the RESET_SEQUENCE.
    assert!(!release.release_callback_in_chain);
    assert!(release.callback_ids.contains(&PLACE_PLAYER_CALLBACK_ID));
    assert!(release.reset_places_player);
    assert_eq!(
        release.reset_sequence,
        vec![
            ResetStep::DeleteChild {
                parent: "piratezep".into(),
                child: "camera1".into()
            },
            ResetStep::DeleteChild {
                parent: "piratezep".into(),
                child: "player".into()
            },
            ResetStep::DeleteChild {
                parent: "piratezep".into(),
                child: "piratefighter".into()
            },
            ResetStep::Callback { id: 1 },
            ResetStep::Callback { id: 0x392 },
            ResetStep::Callback { id: 10 },
            ResetStep::Callback { id: 0x29b },
            ResetStep::AddChild {
                parent: "world1".into(),
                child: "player".into()
            },
            ResetStep::CallAnimation {
                name: "speed_cue".into()
            },
            ResetStep::Other { opcode: 6 },
            ResetStep::Other { opcode: 9 },
            ResetStep::Other { opcode: 9 },
            ResetStep::Other { opcode: 6 },
            ResetStep::Other { opcode: 6 },
            ResetStep::Other { opcode: 6 },
        ]
    );

    // The scan is not vacuous: the release callback exists in M01's mission
    // animations (texdrop and pzep_launch_player.all_done), not in the chain.
    let (mis_path, mis_bytes) = read(MIS_ANIM);
    let sites = callback_sites(&mis_path, &mis_bytes, RELEASE_CALLBACK_ID)
        .expect("the mission animations read");
    let named: Vec<(&str, &str)> = sites
        .iter()
        .map(|s| (s.record.as_str(), s.sequence.as_str()))
        .collect();
    assert!(named.contains(&("texdrop", "")), "{named:?}");
    assert!(
        named.contains(&("pzep_launch_player", "all_done")),
        "{named:?}"
    );
    assert!(
        callback_sites(&path, &bytes, RELEASE_CALLBACK_ID)
            .unwrap()
            .is_empty()
    );
}

#[test]
#[ignore = "requires CS_ENGINE_IMAGE"]
fn accept_parity_m01_atmo_e3_engine_facts_are_the_bytes_the_finding_read() {
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
    // Every `call rel32` in .text whose target is `target`, by address.
    let callers = |target: usize| -> Vec<usize> {
        (0x40_1000..0x60_1000 - 5)
            .filter(|&va| bytes[at(va)] == 0xe8 && call_target(va) == target)
            .collect()
    };
    let calls_within = |from: usize, to: usize| -> Vec<usize> {
        (from..to - 5)
            .filter(|&va| bytes[at(va)] == 0xe8)
            .map(call_target)
            .collect()
    };

    // Every row of the table points into code or data the image has.
    for fact in ENGINE_FACTS {
        assert!((fact.va as usize) < 0x60_1000 + 0x20_000, "{}", fact.id);
    }

    // record-flags-are-instance-0x9c: `test dword ptr [ebx+0x9c], 0x40000`.
    assert!(has(
        0x4ed09e,
        &[0xf7, 0x83, 0x9c, 0, 0, 0, 0x00, 0x00, 0x04, 0x00]
    ));
    assert_eq!(AUTO_RESET_NODE_STATES, 0x40000);

    // stop-animation-executor: opcode 25 -> 0x4ebc80 (slot 0x727de0 + 4*25).
    assert!(has(
        0x4ee297,
        &[0xc7, 0x05, 0x44, 0x7e, 0x72, 0x00, 0x80, 0xbc, 0x4e, 0x00]
    ));
    // opcode 35 CALLBACK -> 0x4ec5e0 (slot 0x727de0 + 4*35 = 0x727e6c).
    assert!(has(
        0x4ee2f6,
        &[0xc7, 0x05, 0x6c, 0x7e, 0x72, 0x00, 0xe0, 0xc5, 0x4e, 0x00]
    ));
    // The dispatcher indexes that table by opcode.
    assert!(has(0x4ecc7f, &[0xff, 0x14, 0x85, 0xe0, 0x7d, 0x72, 0x00]));

    // stop-is-immediate: `fcomp [0x6032c8]` (0.0) on RESET_TIME, then 0x4ed190.
    assert_eq!(float(0x6032c8), 0.0);
    assert!(has(0x4ebbdb, &[0xd9, 0x86, 0xac, 0, 0, 0]));
    assert!(has(0x4ebbe1, &[0xd8, 0x1d, 0xc8, 0x32, 0x60, 0x00]));
    assert_eq!(call_target(0x4ebc44), 0x4ed190);

    // finish-writes-no-node-pose: the callees of 0x4ed190..0x4ed297.
    let finish = calls_within(0x4ed190, 0x4ed297);
    for setter in [0x4d1d50, 0x4d1a30, 0x4d18d0, 0x4d2710] {
        assert!(!finish.contains(&setter), "0x4ed190 calls {setter:#x}");
    }
    for expected in [0x4eba60, 0x4ebae0, 0x4ebb30, 0x4ebbb0] {
        assert!(
            finish.contains(&expected),
            "0x4ed190 must call {expected:#x}"
        );
    }

    // node-restore-is-start-only-and-flag-gated.
    assert_eq!(callers(0x4ed090), vec![0x4ed424]);
    let restore = calls_within(0x4ed090, 0x4ed182);
    for setter in [0x4d1d50, 0x4d1a30, 0x4d18d0] {
        assert!(restore.contains(&setter), "0x4ed090 must call {setter:#x}");
    }

    // reset-sequence-runs-at-start: `mov edi, [esi+0xd0]`, rewind, step.
    assert!(has(0x4ed42c, &[0x8b, 0xbe, 0xd0, 0, 0, 0]));
    assert_eq!(call_target(0x4ed43e), 0x4ebfa0);
    assert_eq!(call_target(0x4ed445), 0x4ecbb0);

    // damage-sequence-runs-from-hit-callbacks-only.
    assert!(has(0x4e71e6, &[0x8b, 0xb7, 0xd4, 0, 0, 0]));
    assert_eq!(callers(0x4e71e0), vec![0x4e7191, 0x4e7274]);
    for (site, target) in [
        (0x523287, 0x4e7150),
        (0x5232cb, 0x4e7150),
        (0x5232ac, 0x4e7220),
        (0x5232e3, 0x4e7220),
    ] {
        assert_eq!(bytes[at(site)], 0x68, "a push at {site:#x}");
        assert_eq!(dword(site + 1) as usize, target);
    }

    // Handler tables of 0x47e080: ids 0..13, 20, and 0x392.. through two tables.
    assert_eq!(dword(0x47f160 + 4 * 10), 0x47e11e);
    assert!(has(0x47e089, &[0x83, 0xf8, 0x14]));
    assert_eq!(bytes[at(0x47f1b8 + (951 - 0x392))], 2);
    assert_eq!(dword(0x47f198 + 4 * 2), 0x47e557);
    assert_eq!(bytes[at(0x47f1b8)], 0);
    assert_eq!(dword(0x47f198), 0x47e503);
    assert_eq!(dword(0x47f160 + 4), 0x47e0cb);
    assert!(has(0x47e2e6, &[0x89, 0x15, 0x0c, 0xc5, 0x71, 0x00]));

    // place-player-from-callback-10.
    assert_eq!(call_target(0x47e154), 0x440ad0);
    assert_eq!(call_target(0x47e15f), 0x47f1f0);
    assert!(has(
        0x440ad0,
        &[0xa1, 0x50, 0xf7, 0x64, 0x00, 0x8b, 0x00, 0xc3]
    ));

    // place-player-routine: the constants and the stores of the cinematic path.
    assert_eq!(float(0x6032e4), 100.0);
    assert_eq!(float(0x608040), -20.0);
    assert!(has(0x47f25f, &[0x38, 0x98, 0x1d, 0x09, 0, 0]));
    assert!(has(0x47f274, &[0xd8, 0x05, 0xe4, 0x32, 0x60, 0x00]));
    assert!(has(
        0x47f28b,
        &[0xc7, 0x80, 0x24, 0x01, 0, 0, 0xcd, 0xcc, 0xcc, 0x3e]
    ));
    assert_eq!(f32::from_bits(0x3ecc_cccd), 0.4);
    assert!(has(0x47f2aa, &[0xd8, 0x0d, 0x40, 0x80, 0x60, 0x00]));
    // The node is written from the body at the tail: veh+0x204 and veh+0x1f8.
    assert_eq!(call_target(0x47f5ea), 0x4d1d50);
    assert_eq!(call_target(0x47f60d), 0x4d1a30);
    assert_eq!(call_target(0x47f61b), 0x489f60);
    assert!(has(0x47f625, &[0x88, 0x98, 0x1d, 0x09, 0, 0]));
    // Two callers outside the intro's callback and the 0x4804xx/0x4805xx pair.
    let mut places = callers(0x47f1f0);
    places.sort_unstable();
    assert_eq!(places, vec![0x4440bf, 0x47e15f, 0x4804d9, 0x4805ef]);

    // release-callback-handler: the only node-pose-to-body copy read.
    assert_eq!(call_target(0x47e565), 0x4cf200);
    assert_eq!(call_target(0x47e578), 0x4cf380);
    assert_eq!(call_target(0x47e69d), 0x4cd6d0);
    assert_eq!(call_target(0x47e6b7), 0x4d1d50);
    assert_eq!(call_target(0x47e6d1), 0x4d1a30);
    assert_eq!(float(0x60803c), -53.644_8);

    // flight-update-skipped-during-intro: `mov eax,[0x71c50c]`, `jne 0x489e30`.
    assert!(has(0x4897dd, &[0xa1, 0x0c, 0xc5, 0x71, 0x00]));
    assert!(has(0x4897f3, &[0x0f, 0x85]));
    assert_eq!(
        (0x4897f3 + 6usize).wrapping_add(dword(0x4897f3 + 2) as i32 as usize),
        0x489e30
    );
}
