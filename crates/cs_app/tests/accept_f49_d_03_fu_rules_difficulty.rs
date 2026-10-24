//! F49-D.03-FU-RULES-DIFF (#1640): how an original Instant Action scenario
//! meets a difficulty.
//!
//! The measurement (`docs/findings/2026-10-24-f49-d-03-fu-rules-difficulty.md`)
//! is that no single difficulty is bound to a scenario: the spawn routine
//! overwrites the `GameIntensity` console variable around each spawn call (the
//! ace from its `ace_skill` label, every enemy group with `1`), and the group
//! `enemy_skill` key is never read. The retail test re-reads that from the
//! owner-supplied decrypted image, independently of the production code; the
//! other tests pin what the production catalog does with the result (the
//! preset difficulty stays unmeasured, citing the evidence).

use std::fs;
use std::path::PathBuf;

use cs_app::ui::instant_action::{
    DIFFICULTY_PER_ACTOR_REASON, IA_DIFFICULTY_PER_ACTOR_CLAIM, retail_instant_action_catalog,
};
use cs_content::instant_action::Declared;

const FINDING: &str = "docs/findings/2026-10-24-f49-d-03-fu-rules-difficulty.md";

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
        match *image_bytes(image, va + offset, 1).first()? {
            0 => return Some(text),
            byte @ 0x20..=0x7e => text.push(char::from(byte)),
            _ => return None,
        }
    }
    None
}

/// The virtual address of every `call rel32` in `.text` that targets `target`.
fn callers_of(image: &[u8], target: u32) -> Vec<u32> {
    const TEXT: u32 = 0x0040_1000;
    const TEXT_LEN: usize = 0x0020_14bd;
    let text = image_bytes(image, TEXT, TEXT_LEN);
    let mut callers = Vec::new();
    for at in 0..TEXT_LEN - 5 {
        if text[at] == 0xe8 {
            let rel = i32::from_le_bytes(text[at + 1..at + 5].try_into().expect("4 bytes"));
            let site = TEXT + at as u32;
            if site.wrapping_add(5).wrapping_add_signed(rel) == target {
                callers.push(site);
            }
        }
    }
    callers
}

fn contains(image: &[u8], needle: &[u8]) -> bool {
    image.windows(needle.len()).any(|window| window == needle)
}

#[test]
#[ignore = "requires CS_ENGINE_IMAGE"]
fn accept_f49_d_03_fu_rules_difficulty_retail_the_setup_binds_the_ace_and_the_groups_not_the_scenario()
 {
    let path = std::env::var("CS_ENGINE_IMAGE").expect("CS_ENGINE_IMAGE must be set");
    let image = fs::read(path).expect("the engine image reads");

    // The difficulty option is the GameIntensity console variable, its
    // pointer kept at 0x64f720, with a getter and a setter.
    assert_eq!(
        c_string(&image, 0x0062_36cc).as_deref(),
        Some("GameIntensity")
    );
    assert_eq!(
        image_bytes(&image, 0x0043_ff27, 5),
        [0x68, 0xcc, 0x36, 0x62, 0x00]
    );
    assert_eq!(
        image_bytes(&image, 0x0043_ff36, 5),
        [0xa3, 0x20, 0xf7, 0x64, 0x00]
    );
    assert_eq!(
        image_bytes(&image, 0x0044_0700, 13),
        [
            0x8b, 0x44, 0x24, 0x04, 0x8b, 0x0d, 0x20, 0xf7, 0x64, 0x00, 0x89, 0x01, 0xc3
        ]
    );
    assert_eq!(
        image_bytes(&image, 0x0044_0710, 7),
        [0xa1, 0x20, 0xf7, 0x64, 0x00, 0x8b, 0x00]
    );

    // Everything that writes it: start-up, the bulk copy, a save and restore
    // around launch, and exactly the two Instant Action spawn sites (the ace
    // and the groups). Nothing in the descriptor readers 0x458d00..0x459c40.
    let setters = callers_of(&image, 0x0044_0700);
    assert_eq!(
        setters,
        [
            0x0041_9185,
            0x0043_ff3e,
            0x0044_04a2,
            0x0045_ace3,
            0x0045_acfc,
            0x0045_b120,
            0x0045_b139
        ]
    );
    assert!(
        setters
            .iter()
            .all(|site| !(0x0045_8d00..0x0045_9c40).contains(site)),
        "no descriptor reader writes GameIntensity"
    );

    // Everything that reads it: launch save, the bulk copy, the two spawn
    // sites, and the spawn call's own two reads.
    assert_eq!(
        callers_of(&image, 0x0044_0710),
        [
            0x0041_8d48,
            0x0044_049c,
            0x0045_acbe,
            0x0045_b100,
            0x0047_cb32,
            0x0047_cb42
        ]
    );

    // The ace: `ace_skill` (0x625620) is read by the scenario reader and
    // compared with novice / veteran / ace, stored at this+0xd0 = 0x718da8,
    // which the spawn routine maps 0/2/else-1 onto GameIntensity around the
    // spawn call 0x47c210.
    assert_eq!(c_string(&image, 0x0062_5620).as_deref(), Some("ace_skill"));
    for (va, label) in [
        (0x0062_562c, "novice"),
        (0x0062_5634, "veteran"),
        (0x0062_563c, "ace"),
    ] {
        assert_eq!(c_string(&image, va).as_deref(), Some(label));
    }
    assert_eq!(
        image_bytes(&image, 0x0045_972b, 5),
        [0x68, 0x20, 0x56, 0x62, 0x00],
        "push \"ace_skill\""
    );
    assert_eq!(
        image_bytes(&image, 0x0045_a222, 10),
        [0xb9, 0xd8, 0x8c, 0x71, 0x00, 0xe8, 0x64, 0xf1, 0xff, 0xff],
        "mov ecx, 0x718cd8; call 0x459390: the scenario record is this = 0x718cd8"
    );
    assert_eq!(
        image_bytes(&image, 0x0045_acc5, 5),
        [0xa1, 0xa8, 0x8d, 0x71, 0x00],
        "mov eax, [0x718da8] = this+0xd0, the ace's skill"
    );
    assert_eq!(
        image_bytes(&image, 0x0045_acf6, 5),
        [0xe8, 0x15, 0x15, 0x02, 0x00],
        "call 0x47c210, the spawn"
    );

    // The groups: the group record's last dword, initialised to 1.
    assert_eq!(
        image_bytes(&image, 0x0045_8ddd, 7),
        [0xc7, 0x45, 0x5c, 1, 0, 0, 0]
    );
    assert_eq!(
        image_bytes(&image, 0x0045_b107, 3),
        [0x8b, 0x45, 0x3c],
        "mov eax, [ebp+0x3c]: the group field"
    );

    // The spawn call reads the value only when its side test passes: the
    // fall-through of `mov eax, 1; test al, al; sete al; test al, al; je`
    // is skipped, and 0x47cb32 is reached only from the `xor eax, eax` path.
    assert_eq!(
        image_bytes(&image, 0x0047_cb24, 12),
        [
            0xb8, 0x01, 0x00, 0x00, 0x00, 0x84, 0xc0, 0x0f, 0x94, 0xc0, 0x84, 0xc0
        ]
    );
    assert_eq!(image_bytes(&image, 0x0047_cb30, 2), [0x74, 0x1f]);
    assert_eq!(
        image_bytes(&image, 0x0047_cc13, 4),
        [0x85, 0xff, 0x0f, 0x84],
        "test edi, edi; je: a zero step skips the scaling"
    );

    // The group `enemy_skill` key is never read: the string is nowhere in the
    // image, while the option name `EnemySkill` and `ace_skill` are.
    assert!(!contains(&image, b"enemy_skill"));
    assert!(!contains(&image, b"difficulty"));
    assert!(contains(&image, b"ace_skill\0"));
    assert!(contains(&image, b"EnemySkill\0"));
}

#[test]
fn accept_f49_d_03_fu_rules_difficulty_the_catalog_cites_the_measurement_and_carries_it_per_actor()
{
    assert_eq!(
        IA_DIFFICULTY_PER_ACTOR_CLAIM,
        "f49.d.03.difficulty-per-actor"
    );
    for needle in [
        "GameIntensity",
        "0x47c210",
        "ace_skill",
        "docs/findings/2026-10-25-f49-d-03-fu-intensity.md",
    ] {
        assert!(
            DIFFICULTY_PER_ACTOR_REASON.contains(needle),
            "{DIFFICULTY_PER_ACTOR_REASON}"
        );
    }
    // The finding the reason cites exists and states the same conclusion.
    let root = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../..");
    let finding = fs::read_to_string(root.join(FINDING)).expect("the finding is committed");
    for needle in [
        "GameIntensity",
        "0x45acbe",
        "0x45b100",
        "`enemy_skill`",
        "not read by the executable",
        "Unmeasured",
    ] {
        assert!(finding.contains(needle), "the finding mentions {needle}");
    }
}

#[test]
#[ignore = "requires CS_GAME_DIR"]
fn accept_f49_d_03_fu_rules_difficulty_retail_no_preset_is_given_a_difficulty() {
    let game_dir = PathBuf::from(std::env::var("CS_GAME_DIR").expect("CS_GAME_DIR must be set"));
    let catalog = retail_instant_action_catalog(&game_dir).expect("the retail catalog assembles");
    assert_eq!(catalog.presets().len(), 30);
    for preset in catalog.presets() {
        match preset.parameters().difficulty() {
            Declared::PerActor { claim_id, reason } => {
                assert_eq!(claim_id.as_str(), IA_DIFFICULTY_PER_ACTOR_CLAIM);
                assert_eq!(reason, DIFFICULTY_PER_ACTOR_REASON);
            }
            other => panic!(
                "{}: a preset difficulty was invented: {other:?}",
                preset.id()
            ),
        }
    }
}
