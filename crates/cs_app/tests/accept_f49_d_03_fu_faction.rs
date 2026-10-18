//! F49-D.03-FU-FACTION (#1582): which faction and survivability each Instant
//! Action actor has.
//!
//! The measurement (`docs/findings/2026-10-18-f49-d-03-fu-faction-survivability.md`)
//! is that the executable's Instant Action setup supplies neither: the
//! descriptor reader asks for no such key, and the spawn call takes only a
//! scene node, a placement and a plane. The retail test re-reads that from the
//! owner-supplied decrypted image, independently of the production code; the
//! other tests pin what the production catalog does with the result (the
//! dimensions stay unmeasured, citing the evidence, and every preset refuses).

use std::fs;
use std::path::PathBuf;

use cs_app::ui::instant_action::{
    FACTION_UNMEASURED_REASON, IA_FACTION_UNMEASURED_CLAIM, IA_SURVIVABILITY_UNMEASURED_CLAIM,
    SURVIVABILITY_UNMEASURED_REASON, retail_instant_action_catalog,
};
use cs_types::content::Resolved;

const FINDING: &str = "docs/findings/2026-10-18-f49-d-03-fu-faction-survivability.md";

/// The image's bytes at virtual address `va`, through its PE section table.
fn image_bytes(image: &[u8], va: u32, len: usize) -> &[u8] {
    let u16_at = |at: usize| u16::from_le_bytes(image[at..at + 2].try_into().expect("2 bytes"));
    let u32_at = |at: usize| u32::from_le_bytes(image[at..at + 4].try_into().expect("4 bytes"));
    let pe = u32_at(0x3c) as usize;
    assert_eq!(&image[pe..pe + 4], b"PE\0\0", "a PE image");
    let sections = usize::from(u16_at(pe + 6));
    let optional = usize::from(u16_at(pe + 20));
    let image_base = u32_at(pe + 24 + 28);
    let table = pe + 24 + optional;
    for index in 0..sections {
        let entry = table + index * 40;
        let virtual_size = u32_at(entry + 8);
        let section_va = image_base + u32_at(entry + 12);
        let raw_size = u32_at(entry + 16);
        let raw = u32_at(entry + 20) as usize;
        if va >= section_va && va - section_va + len as u32 <= virtual_size.min(raw_size) {
            let start = raw + (va - section_va) as usize;
            return &image[start..start + len];
        }
    }
    panic!("virtual address {va:#x} is in no section");
}

/// The NUL-terminated text at `va`, if it is printable ASCII.
fn c_string(image: &[u8], va: u32) -> Option<String> {
    let mut text = String::new();
    for offset in 0..64 {
        let byte = *image_bytes(image, va + offset, 1).first()?;
        match byte {
            0 => return Some(text),
            0x20..=0x7e => text.push(char::from(byte)),
            _ => return None,
        }
    }
    None
}

/// Every string a `push imm32` in `[from, to)` names in the `.rdata` range.
fn pushed_strings(image: &[u8], from: u32, to: u32) -> Vec<String> {
    let code = image_bytes(image, from, (to - from) as usize);
    code.windows(5)
        .filter(|window| window[0] == 0x68)
        .map(|window| u32::from_le_bytes(window[1..5].try_into().expect("4 bytes")))
        .filter(|imm| (0x0062_0000..0x0063_0000).contains(imm))
        .filter_map(|imm| c_string(image, imm))
        .filter(|text| text.len() > 2)
        .collect()
}

#[test]
#[ignore = "requires CS_GAME_DIR and CS_ENGINE_IMAGE"]
fn accept_f49_d_03_fu_faction_retail_the_setup_reads_no_side_or_mortality_key() {
    let _ = PathBuf::from(std::env::var("CS_GAME_DIR").expect("CS_GAME_DIR must be set"));
    let path = std::env::var("CS_ENGINE_IMAGE").expect("CS_ENGINE_IMAGE must be set");
    let image = fs::read(path).expect("the engine image reads");

    // The descriptor readers: record initialiser/reader 0x458d00..0x459c40
    // (group record, scenario record, `ia.zrd` key reads).
    let strings = pushed_strings(&image, 0x0045_8d00, 0x0045_9c40);
    for key in [
        "mission_type",
        "player_plane",
        "num_wingmen",
        "wingman_plane",
        "ace_name",
        "ace_plane",
        "ace_skill",
        "num_enemies",
        "enemy_name",
        "enemy_plane",
        "enemy_accentID",
        "group%d",
    ] {
        assert!(
            strings.iter().any(|s| s == key),
            "the reader asks for {key:?}"
        );
    }
    for text in &strings {
        let lower = text.to_ascii_lowercase();
        for word in [
            "faction", "side", "team", "allegi", "alliance", "mortal", "surviv", "invuln",
            "immortal", "friend", "hostile",
        ] {
            assert!(
                !lower.contains(word),
                "the descriptor readers name {text:?}, which looks like a {word} key"
            );
        }
    }

    // The only per-group value the spawn loop sets around the spawn call is
    // the group record's last dword (+0x5c), which the group initialiser sets
    // to 1 and no reader writes, and it goes to the GameIntensity variable.
    assert_eq!(
        image_bytes(&image, 0x0045_8ddd, 7),
        [0xc7, 0x45, 0x5c, 1, 0, 0, 0],
        "group initialiser: mov [ebp+0x5c], 1"
    );
    assert_eq!(
        c_string(&image, 0x0062_36cc).as_deref(),
        Some("GameIntensity")
    );
    assert_eq!(
        image_bytes(&image, 0x0043_ff27, 5),
        [0x68, 0xcc, 0x36, 0x62, 0x00],
        "push \"GameIntensity\""
    );
    assert_eq!(
        image_bytes(&image, 0x0043_ff36, 5),
        [0xa3, 0x20, 0xf7, 0x64, 0x00],
        "mov [0x64f720], eax: the variable's address"
    );
    assert_eq!(
        image_bytes(&image, 0x0044_0700, 13),
        [
            0x8b, 0x44, 0x24, 0x04, 0x8b, 0x0d, 0x20, 0xf7, 0x64, 0x00, 0x89, 0x01, 0xc3
        ],
        "the setter 0x440700 writes through that pointer"
    );
    // The spawn loop: read the old value (0x440710), map the group field
    // 0/2/other to 0/2/1, set it (0x440700), spawn (0x47c210), restore.
    assert_eq!(
        image_bytes(&image, 0x0045_b100, 5),
        [0xe8, 0x0b, 0x56, 0xfe, 0xff],
        "call 0x440710"
    );
    assert_eq!(
        image_bytes(&image, 0x0045_b107, 3),
        [0x8b, 0x45, 0x3c],
        "mov eax, [ebp+0x3c]: the group field"
    );
    assert_eq!(
        image_bytes(&image, 0x0045_b133, 5),
        [0xe8, 0xd8, 0x10, 0x02, 0x00],
        "call 0x47c210, the spawn, with (node, placement, 0, 0) and no side"
    );
}

#[test]
fn accept_f49_d_03_fu_faction_the_catalog_cites_the_measurement_and_stays_unmeasured() {
    assert_eq!(IA_FACTION_UNMEASURED_CLAIM, "f49.d.03.unmeasured.faction");
    assert_eq!(
        IA_SURVIVABILITY_UNMEASURED_CLAIM,
        "f49.d.03.unmeasured.survivability"
    );
    for reason in [FACTION_UNMEASURED_REASON, SURVIVABILITY_UNMEASURED_REASON] {
        assert!(reason.contains("0x47c210"), "{reason}");
        assert!(reason.contains(FINDING), "{reason}");
    }
    // The finding the reasons cite exists and states the same conclusion.
    let root = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../..");
    let finding = fs::read_to_string(root.join(FINDING)).expect("the finding is committed");
    for needle in [
        "f49.d.03.unmeasured.faction",
        "f49.d.03.unmeasured.survivability",
        "GameIntensity",
        "0x47c210",
        "unmeasured",
    ] {
        assert!(finding.contains(needle), "the finding mentions {needle}");
    }
}

#[test]
#[ignore = "requires CS_GAME_DIR"]
fn accept_f49_d_03_fu_faction_retail_no_actor_is_given_a_guessed_faction_or_mortality() {
    let game_dir = PathBuf::from(std::env::var("CS_GAME_DIR").expect("CS_GAME_DIR must be set"));
    let catalog = retail_instant_action_catalog(&game_dir).expect("the retail catalog assembles");
    assert_eq!(catalog.presets().len(), 30);
    for preset in catalog.presets() {
        for actor in preset.parameters().roster().actors() {
            assert!(
                matches!(actor.faction(), Resolved::Unknown { .. }),
                "{}: {} has no measured faction",
                preset.id(),
                actor.role().unwrap_or("?")
            );
            assert!(
                matches!(actor.survivability(), Resolved::Unknown { .. }),
                "{}: {} has no measured survivability",
                preset.id(),
                actor.role().unwrap_or("?")
            );
        }
    }
}
