//! Task #2117 against the owner's engine image: what removes a projectile at
//! the end of `RANGE`, what the flare (`wep_15`, no `RANGE` key) gets instead,
//! what a session teardown does to projectiles in flight, and whether
//! `CLUSTER_SIZE` shapes a launch.
//!
//! Task test prefix: `accept_t2117_`. Findings:
//! `docs/findings/2026-10-27-t2117-guided-rocket-lifetime-end.md`.
//!
//! Follow-up of #454 (`t454.limit.session_change`,
//! `t454.limit.range_end_removal`, `t454.limit.swarm`). The pins are byte
//! windows and byte-level inventories of the owner-supplied decrypted
//! executable (`$CS_ENGINE_IMAGE`, base `0x400000`): static-analysis evidence,
//! never original-run behaviour, and no code is printed or committed beyond the
//! byte strings below.

use std::path::PathBuf;

/// The image base the executable's virtual addresses are relative to.
const IMAGE_BASE: u64 = 0x40_0000;
/// First byte of `.text` (file offset equals RVA there) and its length.
const TEXT_START: u64 = 0x40_1000;
const TEXT_LEN: usize = 0x20_14bd;

/// Byte windows every measured fact rests on: address, bytes, what they are.
const IMAGE_PINS: [(u64, &[u8], &str); 14] = [
    (
        0x5b_01e6,
        &[
            0xd9, 0x86, 0x64, 0x06, 0x00, 0x00, 0xd8, 0x5f, 0x1c, 0xdf, 0xe0,
        ],
        "step: the distance flown (projectile +0x664) is compared with the weapon record's +0x1c, RANGE",
    ),
    (
        0x5b_01f1,
        &[
            0xf6, 0xc4, 0x01, 0x8b, 0x47, 0x74, 0x75, 0x1d, 0xf6, 0xc4, 0x80, 0x74, 0x08, 0xa8,
            0x80, 0x0f, 0x84, 0xb4, 0x00, 0x00, 0x00, 0xa9, 0x00, 0x00, 0x00, 0x02, 0x0f, 0x84,
            0x07, 0x01, 0x00, 0x00, 0xe9, 0xa4, 0x00, 0x00, 0x00,
        ],
        "end of RANGE: LOCK_ON (0x8000) with EXPIRES (0x80) clear detonates (0x5b02ba); otherwise DETONATE_AT_RANGE (0x2000000) set detonates, clear jumps to the silent removal 0x5b0318",
    ),
    (
        0x5b_0310,
        &[
            0xe8, 0x8b, 0xc0, 0xff, 0xff, 0x83, 0xc4, 0x08, 0xc7, 0x46, 0x20, 0x00, 0x00, 0x00,
            0x00,
        ],
        "0x5b030e..0x5b0318: the detonation call 0x5ac3a0 falls into `[node+0x20] = 0`, which the silent path 0x5b0318 shares",
    ),
    (
        0x5a_d80b,
        &[0xc7, 0x43, 0x1c, 0x00, 0x00, 0xfa, 0x43],
        "weapon record parse default: record +0x1c (RANGE) = 500.0f before any key is read",
    ),
    (
        0x5a_df90,
        &[
            0x68, 0xf4, 0xfe, 0x62, 0x00, 0x56, 0xe8, 0xf5, 0xc0, 0xfc, 0xff, 0x33, 0xff, 0x83,
            0xc4, 0x08, 0x3b, 0xc7, 0x74, 0x17,
        ],
        "RANGE key lookup (string 0x62fef4); an absent key skips the store",
    ),
    (
        0x5a_dfb5,
        &[0x89, 0x43, 0x1c, 0xd9, 0x5b, 0x20],
        "the store of RANGE to +0x1c and RANGE squared to +0x20, executed only when the key is present",
    ),
    (
        0x5a_f73c,
        &[0x39, 0x5e, 0x20, 0x0f, 0x84, 0x9d, 0x01, 0x00, 0x00],
        "step: a node whose state +0x20 is zero is skipped",
    ),
    (
        0x5a_f917,
        &[
            0x8b, 0x46, 0x20, 0x85, 0xc0, 0x74, 0x11, 0x83, 0xf8, 0x01, 0x75, 0x24, 0x56, 0x53,
            0xe8, 0x46, 0x0e, 0x00, 0x00, 0x83, 0xc4, 0x08, 0xeb, 0x18, 0x8b, 0x06, 0x56, 0x53,
            0x89, 0x07,
        ],
        "cleanup walk 0x5af900: state 0 unlinks the node and releases it through 0x5b08d0; state 1 runs the per-frame update 0x5b0770",
    ),
    (
        0x5b_08d0,
        &[
            0xa1, 0xf8, 0xe6, 0x9b, 0x00, 0x56, 0x8b, 0x74, 0x24, 0x0c, 0x39, 0x86, 0x6c, 0x06,
            0x00, 0x00, 0x74, 0x0b, 0x40, 0x89, 0x86, 0x6c, 0x06, 0x00, 0x00, 0x33, 0xc0, 0x5e,
            0xc3,
        ],
        "release 0x5b08d0: the first call stamps the node with the next frame number and returns 0; the node is released one frame later",
    ),
    (
        0x5a_e810,
        &[
            0x6a, 0x00, 0xe8, 0x09, 0x00, 0x00, 0x00, 0x83, 0xc4, 0x04, 0xc3,
        ],
        "teardown entry 0x5ae810: `push 0; call 0x5ae820`",
    ),
    (
        0x5a_e6cc,
        &[
            0x8d, 0xb0, 0x60, 0x01, 0x00, 0x00, 0x8b, 0x06, 0x3b, 0xc5, 0x74, 0x14, 0x8b, 0x48,
            0x40, 0x50, 0x89, 0x0e, 0xe8, 0x6d, 0x93, 0xf2, 0xff,
        ],
        "stop 0x5ae690(0): walks each weapon record's +0x160 list and destroys its entries (0x4d7a50)",
    ),
    (
        0x5a_e711,
        &[
            0x8b, 0x15, 0x94, 0xd7, 0xa1, 0x00, 0x8b, 0x74, 0x17, 0x0c, 0x3b, 0xf5, 0x74, 0x1f,
            0x66, 0x83, 0x7e, 0x54, 0x01, 0x75, 0x0f, 0x8b, 0x46, 0x58, 0x8b, 0x08, 0x56, 0x51,
            0xe8, 0x9e, 0xef, 0xf1, 0xff, 0x83, 0xc4, 0x08, 0x56, 0xe8, 0x15, 0x93,
        ],
        "stop 0x5ae690(0): over the 0x694-byte pool slots, destroys the visual at slot +0xc; no projectile callback is called",
    ),
    (
        0x4b_adc8,
        &[0x8b, 0x90, 0x10, 0x02, 0x00, 0x00, 0x8b, 0x42, 0x0c],
        "0x4bad90 `cluster size of weapon id`: the descriptor [weapon+0x210], field +0xc, CLUSTER_SIZE",
    ),
    (
        0x4a_b161,
        &[
            0x8b, 0x86, 0x10, 0x02, 0x00, 0x00, 0x3b, 0xc3, 0x74, 0x07, 0x8a, 0x48, 0x0c, 0x84,
            0xc9, 0x75,
        ],
        "the only other reader of descriptor +0xc: a byte test, non-zero enables the mount (+0x16c)",
    ),
];

fn image() -> Vec<u8> {
    let path = std::env::var_os("CS_ENGINE_IMAGE").expect(
        "CS_ENGINE_IMAGE is not set: this measurement needs the owner-supplied decrypted \
         executable",
    );
    std::fs::read(&path).expect("the engine image reads")
}

fn at(image: &[u8], address: u64, len: usize) -> &[u8] {
    let offset = usize::try_from(address - IMAGE_BASE).expect("offset");
    &image[offset..offset + len]
}

fn text(image: &[u8]) -> &[u8] {
    at(image, TEXT_START, TEXT_LEN)
}

/// Every virtual address of an `e8 rel32` whose target is `target`.
fn direct_calls(text: &[u8], target: u64) -> Vec<u64> {
    let mut found = Vec::new();
    for i in 0..text.len().saturating_sub(4) {
        if text[i] != 0xe8 {
            continue;
        }
        let rel = i32::from_le_bytes([text[i + 1], text[i + 2], text[i + 3], text[i + 4]]);
        let address = TEXT_START + i as u64;
        if address.wrapping_add(5).wrapping_add_signed(i64::from(rel)) == target {
            found.push(address);
        }
    }
    found
}

/// Every offset in `.text` where the four bytes `88 06 00 00` (displacement
/// `0x688`, the projectile's callback slot) occur.
fn slot_688_displacements(text: &[u8]) -> Vec<u64> {
    (0..text.len().saturating_sub(3))
        .filter(|&i| text[i..i + 4] == [0x88, 0x06, 0x00, 0x00])
        .map(|i| TEXT_START + i as u64)
        .collect()
}

fn assert_no_data_word(image: &[u8], target: u64) {
    let absolute = (target as u32).to_le_bytes();
    assert!(
        !image.windows(4).any(|window| window == absolute),
        "0x{target:x} is stored as data: an indirect caller may exist"
    );
}

/// The code half: byte windows and the closed caller lists.
#[test]
#[ignore = "requires CS_ENGINE_IMAGE"]
fn accept_t2117_image_pins_the_lifetime_end_and_teardown() {
    let image = image();
    for (address, bytes, meaning) in IMAGE_PINS {
        assert_eq!(
            at(&image, address, bytes.len()),
            bytes,
            "0x{address:x} no longer carries: {meaning}"
        );
    }
    let text = text(&image);

    // The step and the cleanup walk each have exactly one caller, the
    // per-frame driver; the release has exactly one, the cleanup walk.
    assert_eq!(direct_calls(text, 0x5a_f720), [0x5a_c503]);
    assert_eq!(direct_calls(text, 0x5a_f900), [0x5a_c531]);
    assert_eq!(direct_calls(text, 0x5b_08d0), [0x5a_f935]);

    // Teardown: the stop 0x5ae690 is called by the initialiser (argument 1)
    // and by the teardown 0x5ae820 (argument 0) only; the teardown is reached
    // from 0x5ae810 and 0x5ae950 only, which have one caller each.
    assert_eq!(direct_calls(text, 0x5a_e690), [0x5a_e5ec, 0x5a_e827]);
    assert_eq!(direct_calls(text, 0x5a_e820), [0x5a_e812, 0x5a_e955]);
    assert_eq!(direct_calls(text, 0x5a_e810), [0x5a_d610]);
    assert_eq!(direct_calls(text, 0x5a_e950), [0x46_4a96]);
    for target in [
        0x5a_e690, 0x5a_e810, 0x5a_e820, 0x5a_e950, 0x5a_f720, 0x5a_f900,
    ] {
        assert_no_data_word(&image, target);
    }

    // The projectile callback slot (+0x688) is called through exactly once,
    // in the step (`call eax` at 0x5af757); nothing in the stop or teardown
    // (0x5ae690..0x5ae960) names the displacement.
    assert_eq!(at(&image, 0x5a_f757, 2), [0xff, 0xd0]);
    assert!(
        slot_688_displacements(text)
            .iter()
            .all(|address| !(0x5a_e690..0x5a_e960).contains(address)),
        "the stop or teardown touches the +0x688 callback slot"
    );

    // CLUSTER_SIZE: the getter 0x4bad90 has two callers, the launch routine's
    // six callers never reach it, and the descriptor field is not read by the
    // launch routine 0x5aef40 (its 0x800 bytes name `[reg+0x210]` never).
    assert_eq!(direct_calls(text, 0x4b_ad90), [0x44_4340, 0x4b_25b1]);
    assert_no_data_word(&image, 0x4b_ad90);
    let launch = at(&image, 0x5a_ef40, 0x800);
    assert!(
        !launch.windows(4).any(|w| w == [0x10, 0x02, 0x00, 0x00]),
        "the launch routine reads a weapon's +0x210 descriptor"
    );
}

/// The data half: the flare declares no `RANGE`, the other eleven guided
/// records do; `CLUSTER_SIZE` is declared by the records the census names.
#[test]
#[ignore = "requires CS_GAME_DIR"]
fn accept_t2117_flare_declares_no_range_and_cluster_census() {
    let dir = std::env::var_os("CS_GAME_DIR")
        .expect("CS_GAME_DIR is not set: this measurement needs the original installation");
    let blasts = cs_content::ordnance::read_original_rocket_blasts(&PathBuf::from(dir))
        .expect("weapons.zrd reads");
    let guided: Vec<_> = blasts.iter().filter(|b| b.lock_on.is_some()).collect();
    assert_eq!(guided.len(), 12);
    let without_range: Vec<&str> = guided
        .iter()
        .filter(|b| b.range.is_none())
        .map(|b| b.record.as_str())
        .collect();
    assert_eq!(without_range, ["wep_15"]);
    let clustered: Vec<&str> = blasts
        .iter()
        .filter(|b| b.keys.contains("CLUSTER_SIZE"))
        .map(|b| b.record.as_str())
        .collect();
    assert_eq!(clustered, CLUSTER_RECORDS);
}

/// Records among the rocket records `read_original_rocket_blasts` returns
/// that declare `CLUSTER_SIZE`.
const CLUSTER_RECORDS: [&str; 15] = [
    "wep_04", "wep_05", "wep_06", "wep_07", "wep_08", "wep_09", "wep_10", "wep_11", "wep_12",
    "wep_13", "wep_14", "wep_15", "wep_24", "wep_26", "wep_27",
];
