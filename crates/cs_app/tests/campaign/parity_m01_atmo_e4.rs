//! PARITY-M01-ATMO-E4 (#1679): the Pandora's parent node and motion during the
//! M01 launch cinematic (finding
//! `docs/findings/2026-10-19-m01-launch-parent-node.md`).
//!
//! The synthetic members run in CI. The retail members read `$CS_GAME_DIR`
//! (never write it) through production code and fail loudly without it; one
//! reads `$CS_ENGINE_IMAGE` for the instruction bytes the finding rests on.

use std::path::PathBuf;

use cs_app::cinematics::launch_end_pose::LAUNCH_INTRO_ANIMATION;
use cs_app::cinematics::launch_parent::{
    Ancestry, AncestryLink, LaunchParentError, NodeRole, PANDORA_NODE, PARENT_UNKNOWNS,
    ZEPPELIN_ENGINE_FACTS, events_naming_node, measure_ancestry,
};
use cs_app::cinematics::launch_release::callback_sites;
use cs_types::install::RelativePath;

const GAMEZ: &str = "ZBD/C3/gamez.zbd";
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

fn link(kind: &'static str, translation: Option<[f32; 3]>) -> AncestryLink {
    AncestryLink {
        index: 0,
        name: kind.to_owned(),
        kind,
        translation,
        rotation: translation,
        lists_previous_as_child: true,
    }
}

#[test]
fn accept_parity_m01_atmo_e4_synthetic_refusals_and_ancestry_predicates() {
    let error = measure_ancestry("zbd/c3/gamez.zbd", &[0u8; 64], PANDORA_NODE)
        .expect_err("bytes that are no GameZ container have no ancestry");
    assert!(
        matches!(error, LaunchParentError::Nodes(_)),
        "unexpected refusal {error}"
    );
    let path = RelativePath::new("zbd/c3/cam_anim.zbd").expect("a relative path");
    assert!(events_naming_node(&path, &[0u8; 64], PANDORA_NODE).is_err());

    // A chain that ends in a world node whose ancestors store no transform.
    let plain = Ancestry {
        chain: vec![link("object3d", Some([1.0, 2.0, 3.0])), link("world", None)],
        same_name_count: 1,
    };
    assert!(plain.ends_in_world_node());
    assert!(plain.ancestors_store_no_transform());
    // An ancestor with a stored translation breaks the identity-frame claim,
    // and a chain that ends in a non-world root is not a world chain.
    let moved = Ancestry {
        chain: vec![
            link("object3d", Some([0.0; 3])),
            link("object3d", Some([0.0, 5.0, 0.0])),
        ],
        same_name_count: 1,
    };
    assert!(!moved.ends_in_world_node());
    assert!(!moved.ancestors_store_no_transform());

    assert!(NodeRole::Motion.writes_transform());
    assert!(NodeRole::RotateState.writes_transform());
    assert!(!NodeRole::ActiveState.writes_transform());
    assert!(!NodeRole::ParentOfChild.writes_transform());
}

#[test]
fn accept_parity_m01_atmo_e4_unknowns_and_facts_are_named() {
    let ids: Vec<&str> = PARENT_UNKNOWNS.iter().map(|u| u.id).collect();
    for required in [
        "first-frame-order",
        "other-enablers",
        "post-intro-motion",
        "no-run",
    ] {
        assert!(
            ids.contains(&required),
            "the unknown `{required}` is missing"
        );
    }
    for unknown in PARENT_UNKNOWNS {
        assert!(!unknown.what.is_empty() && !unknown.affects.is_empty());
        assert!(
            !unknown.resolver.is_empty(),
            "{} has no resolver",
            unknown.id
        );
    }
    for fact in ZEPPELIN_ENGINE_FACTS {
        assert!(
            fact.va >= 0x40_1000 && !fact.claim.is_empty(),
            "{}",
            fact.id
        );
    }
}

#[test]
#[ignore = "requires CS_GAME_DIR"]
fn accept_parity_m01_atmo_e4_retail_pandora_hangs_directly_under_the_world_root() {
    let (path, bytes) = read(GAMEZ);
    let ancestry = measure_ancestry(&path.logical_key(), &bytes, PANDORA_NODE)
        .expect("the shared world container holds the Pandora");
    assert_eq!(ancestry.same_name_count, 1, "the name is unique");
    let names: Vec<(&str, &str)> = ancestry
        .chain
        .iter()
        .map(|l| (l.name.as_str(), l.kind))
        .collect();
    assert_eq!(names, [("piratezep", "object3d"), ("world1", "world")]);
    assert!(ancestry.chain.iter().all(|l| l.lists_previous_as_child));
    assert!(ancestry.ends_in_world_node());
    assert!(ancestry.ancestors_store_no_transform());
    // The Pandora's own record stores the identity transform: the placement
    // is `zeppelins.zrd`'s alone.
    let own = &ancestry.chain[0];
    assert_eq!(own.translation, Some([0.0; 3]));
    assert_eq!(own.rotation, Some([0.0; 3]));
}

#[test]
#[ignore = "requires CS_GAME_DIR"]
fn accept_parity_m01_atmo_e4_retail_no_animation_writes_the_pandoras_transform() {
    let mut all = Vec::new();
    for container in [CAM_ANIM, MIS_ANIM] {
        let (path, bytes) = read(container);
        let events = events_naming_node(&path, &bytes, PANDORA_NODE)
            .unwrap_or_else(|e| panic!("{container}: {e}"));
        all.extend(events.into_iter().map(|e| (container, e)));
    }
    // The intro re-parents the player and the cameras under the Pandora, so
    // the reader does see it named.
    assert!(
        all.iter().any(|(c, e)| *c == CAM_ANIM
            && e.record == LAUNCH_INTRO_ANIMATION
            && e.role == NodeRole::ParentOfChild),
        "generic_intro names the Pandora as a parent: {all:?}"
    );
    let writers: Vec<_> = all
        .iter()
        .filter(|(_, e)| e.role.writes_transform())
        .collect();
    assert!(
        writers.is_empty(),
        "an animation writes the Pandora's transform: {writers:?}"
    );

    // The callbacks the finding's engine chain rests on sit where it says.
    let (path, bytes) = read(CAM_ANIM);
    let sites = |id| callback_sites(&path, &bytes, id).expect("the container reads");
    let enabling = sites(0x392);
    assert!(
        enabling.iter().any(|s| s.record == LAUNCH_INTRO_ANIMATION),
        "{enabling:?}"
    );
    let disabling = sites(0x391);
    assert!(
        disabling.iter().any(|s| s.record == "player_setup"),
        "{disabling:?}"
    );
    let remembering = sites(20);
    assert!(
        remembering
            .iter()
            .any(|s| s.record == LAUNCH_INTRO_ANIMATION && s.sequence == "callback_sequence"),
        "{remembering:?}"
    );
}

#[test]
#[ignore = "requires CS_ENGINE_IMAGE"]
fn accept_parity_m01_atmo_e4_engine_facts_are_the_bytes_the_finding_read() {
    let image = cs_content::coordinates::load_engine_image().expect("CS_ENGINE_IMAGE names it");
    assert_eq!(
        image.digest,
        cs_content::coordinates::original_image_digest()
    );
    let bytes = image.bytes.as_slice();
    let at = |va: usize| va - 0x40_0000;
    let dword = |va: usize| u32::from_le_bytes(bytes[at(va)..at(va) + 4].try_into().unwrap());
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
    for fact in ZEPPELIN_ENGINE_FACTS {
        assert!((fact.va as usize) < 0x60_1000, "{}", fact.id);
    }

    // zeppelin-loader-slots: position -> +0x20, max_speed -> +0x9c, max_accel -> +0xa8.
    assert!(has(0x4bda75, &[0x89, 0x53, 0x20]));
    assert!(has(0x4bda7e, &[0x89, 0x53, 0x24]));
    assert!(has(0x4bda8d, &[0x89, 0x4b, 0x28]));
    assert!(has(0x4bdad6, &[0x89, 0x93, 0x9c, 0, 0, 0]));
    assert!(has(0x4bdaf4, &[0x89, 0x8b, 0xa8, 0, 0, 0]));
    // Their key names are the strings the loader passes.
    let text_at = |va: usize| {
        let tail = &bytes[at(va)..];
        let end = tail.iter().position(|&b| b == 0).unwrap();
        std::str::from_utf8(&tail[..end]).unwrap().to_owned()
    };
    assert_eq!(text_at(0x62b6dc), "position");
    assert_eq!(text_at(0x62b6f4), "max_speed");
    assert_eq!(text_at(0x62b700), "max_accel");

    // zeppelin-constructor-starts-idle: eax = 0, then the three flag bytes
    // and the speed word are stored from it.
    assert!(has(0x4bd478, &[0x33, 0xc0]));
    assert!(has(
        0x4bd483,
        &[0x88, 0x46, 0x04, 0x88, 0x46, 0x05, 0x88, 0x46, 0x06]
    ));
    assert!(has(0x4bd52b, &[0x89, 0x86, 0xa4, 0, 0, 0]));

    // zeppelin-enable-all: 0x4bd370 stores dl = 1 to +0x4, 0x4bd390 dl = 0.
    assert!(has(0x4bd379, &[0xb2, 0x01]));
    assert!(has(0x4bd37d, &[0x88, 0x56, 0x04]));
    assert!(has(0x4bd399, &[0x32, 0xd2]));
    assert!(has(0x4bd39d, &[0x88, 0x56, 0x04]));
    assert_eq!(callers(0x4bd370), vec![0x41f2e7, 0x496610, 0x4966aa]);
    assert_eq!(callers(0x4bd390), vec![0x41f256]);

    // zeppelin-enable-callbacks: the handlers call 0x41f250 / 0x41f2e0.
    assert_eq!(call_target(0x47e4b8), 0x41f250);
    assert_eq!(call_target(0x47e510), 0x41f2e0);
    assert!(callers(0x41f250).contains(&0x47e4b8));
    assert!(callers(0x41f2e0).contains(&0x47e510));

    // zeppelin-update-needs-enable-byte: `mov cl,[esi+4]; test cl,cl; ...;
    // je 0x4bfc6f`.
    assert!(has(0x4bfa0f, &[0x8a, 0x4e, 0x04, 0x84, 0xc9]));
    assert!(has(0x4bfa16, &[0x0f, 0x84, 0x53, 0x02, 0, 0]));
    assert_eq!(0x4bfa16 + 6 + 0x253, 0x4bfc6f);

    // zeppelin-moves-by-speed: position += speed * dt, then the node is
    // written; the speed has one non-initialising writer.
    assert!(has(0x4bf7af, &[0xd8, 0x89, 0xa4, 0, 0, 0]));
    assert!(has(0x4bf7c2, &[0xd8, 0x41, 0x20]));
    assert_eq!(call_target(0x4bf7de), 0x4bf930);
    assert_eq!(call_target(0x4bf93a), 0x4bf9b0);
    assert_eq!(call_target(0x4bf9c0), 0x4d1d50);
    assert!(has(0x4bf27e, &[0xd9, 0x91, 0xa4, 0, 0, 0]));
    assert!(has(0x4bf29c, &[0xd9, 0x91, 0xa4, 0, 0, 0]));
    assert!(has(0x4c0adc, &[0x89, 0x8e, 0xa4, 0, 0, 0]));

    // zeppelin-update-only-in-world-update.
    assert_eq!(callers(0x4bd3b0), vec![0x489c75]);
    assert_eq!(call_target(0x4bd3be), 0x4bf9d0);
    assert_eq!(callers(0x4bf9d0), vec![0x4bd3be]);

    // world-update-skipped-during-intro: `mov eax,[0x71c50c]` ... `jne 0x489e30`.
    assert!(has(0x4897dd, &[0xa1, 0x0c, 0xc5, 0x71, 0x00]));
    assert!(has(0x4897f3, &[0x0f, 0x85, 0x37, 0x06, 0, 0]));
    assert_eq!(0x4897f3 + 6 + 0x637, 0x489e30);
    // No `call`/`jmp rel32` from outside the function lands inside the span.
    for va in 0x40_1000..0x60_1000 - 5 {
        let opcode = bytes[at(va)];
        if opcode != 0xe8 && opcode != 0xe9 {
            continue;
        }
        let target = (va + 5).wrapping_add(dword(va + 1) as i32 as usize);
        if (0x4897f9..0x489e30).contains(&target) {
            assert!(
                (0x4897c0..0x489e30).contains(&va),
                "{va:#x} jumps into the world update's body"
            );
        }
    }
}
