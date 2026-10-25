//! Task #2116 against the owner's engine image: where a launched projectile's
//! target pointer (`+0x70`, with its companion `+0x74`) is first assigned, and
//! whether anything re-assigns it in flight.
//!
//! Task test prefix: `accept_t2116_`. Findings:
//! `docs/findings/2026-10-25-t2116-projectile-target-writers.md`.
//!
//! Follow-up of #454's `t454.limit.target_assignment`. The pins are byte
//! windows and a byte-level inventory of the owner-supplied decrypted
//! executable (`$CS_ENGINE_IMAGE`, base `0x400000`). They are static-analysis
//! evidence, never original-run behaviour; no code is printed or committed
//! beyond the byte strings below.

use std::collections::BTreeSet;
use std::path::PathBuf;

/// The image base the executable's virtual addresses are relative to.
const IMAGE_BASE: u64 = 0x40_0000;
/// First byte of `.text` (file offset equals RVA there) and its length.
const TEXT_START: u64 = 0x40_1000;
const TEXT_LEN: usize = 0x20_14bd;

/// The launch routine, whose zeroing store is the only one a launch makes.
const LAUNCH: u64 = 0x5a_ef40;

/// Every `mov [reg+0x70], src` and `mov dword [reg+0x70], imm32` with a
/// register base other than `esp` in `.text` (encodings `89 /r 70` with
/// mod=01 and `c7 /0 70`). A store through `esp` is a stack local, never a
/// heap projectile node. The word store at `0x502d64` carries a `66` prefix,
/// so the raw match sits one byte later.
const POINTER_BASE_STORES: [u64; 65] = [
    0x4252ab, 0x42f6e4, 0x42fa21, 0x436d4f, 0x4373d1, 0x437832, 0x438ef6, 0x4417a7, 0x4417b8,
    0x451892, 0x453819, 0x45906c, 0x464212, 0x46def9, 0x470b20, 0x477f79, 0x478ab2, 0x47a27d,
    0x4a9b93, 0x4a9e9b, 0x4a9ea3, 0x4b1a1b, 0x4b1b51, 0x4bd4e8, 0x4bd728, 0x4cc5f2, 0x4ce85f,
    0x4ce86f, 0x4d7e7a, 0x4da1a9, 0x4de8a5, 0x4e2a86, 0x4e2a96, 0x4e2aa6, 0x4e3829, 0x4e3839,
    0x4e3849, 0x4eda4c, 0x4f4fed, 0x4f51ef, 0x4f8ed0, 0x4f90b9, 0x502d65, 0x502d82, 0x50eb41,
    0x50ed6e, 0x520949, 0x523248, 0x54e80d, 0x54eaf1, 0x54ffdb, 0x55016a, 0x57b5e5, 0x57b5ee,
    0x5ac0f1, 0x5ad95e, 0x5af610, 0x5b6df5, 0x5bd1a8, 0x5bd24e, 0x5c8e39, 0x5e52f7, 0x5eb4ab,
    0x5ebcdf, 0x5ebe1f,
];

/// The six direct calls of the launch routine, and the bytes that follow each
/// call: the result is only counted, tested and handed to the projectile
/// setup `0x441830` (or discarded); none stores through it.
const LAUNCH_CALLERS: [(u64, &[u8]); 6] = [
    (
        0x4ab3cc,
        &[
            0x83, 0xc4, 0x38, 0x3b, 0xc3, 0x74, 0x34, 0x8b, 0x8e, 0x60, 0x01, 0x00, 0x00, 0x50,
            0x51, 0x8d, 0x46, 0x08, 0x50, 0xe8, 0x47, 0x64, 0xf9, 0xff,
        ],
    ),
    (
        0x4b6e7b,
        &[
            0x83, 0xc4, 0x28, 0x85, 0xc0, 0x74, 0x20, 0x8b, 0x55, 0xe4, 0x50, 0x52, 0x8d, 0x46,
            0x08, 0x50, 0xe8, 0x9b, 0xa9, 0xf8, 0xff, 0xa1, 0x98, 0xc2,
        ],
    ),
    (
        0x4b7289,
        &[
            0x8b, 0xd8, 0x83, 0xc4, 0x28, 0x85, 0xdb, 0x74, 0x0e, 0x53, 0x8d, 0x56, 0x08, 0x57,
            0x52, 0xe8, 0x8e, 0xa5, 0xf8, 0xff, 0x83, 0xc4, 0x0c, 0x8b,
        ],
    ),
    (
        0x4b7559,
        &[
            0x83, 0xc4, 0x28, 0x85, 0xc0, 0x0f, 0x84, 0x3e, 0xf9, 0xff, 0xff, 0x50, 0x8d, 0x56,
            0x08, 0x53, 0x52, 0xe8, 0xbc, 0xa2, 0xf8, 0xff, 0x83, 0xc4,
        ],
    ),
    (
        0x4c01e0,
        &[
            0x83, 0xc4, 0x30, 0xe8, 0xe3, 0x08, 0xf8, 0xff, 0x85, 0xc0, 0x74, 0x14, 0x8b, 0x4d,
            0xdc, 0x8b, 0x55, 0xe0, 0x8d, 0x47, 0x20, 0x89, 0x08, 0x8b,
        ],
    ),
    (
        0x4c0d59,
        &[
            0x83, 0xc4, 0x34, 0x8b, 0x46, 0x60, 0x83, 0xc5, 0x04, 0x43, 0x3b, 0xe8, 0x75, 0xa9,
            0x5f, 0x5e, 0x5d, 0x5b, 0x83, 0xc4, 0x0c, 0xc2, 0x08, 0x00,
        ],
    ),
];

/// `(virtual address, bytes, what the window carries)`.
const IMAGE_PINS: [(u64, &[u8], &str); 6] = [
    (
        0x5af581,
        &[0x33, 0xdb],
        "launch: ebx is zeroed (xor ebx, ebx) on the way to the tail stores",
    ),
    (
        0x5af60a,
        &[
            0x89, 0x9f, 0x3c, 0x06, 0x00, 0x00, 0x89, 0x5f, 0x70, 0x89, 0x5f, 0x74,
        ],
        "launch tail: +0x63c, then target +0x70 and companion +0x74 stored from the zeroed ebx",
    ),
    (
        0x5ac0e2,
        &[
            0x8b, 0x82, 0x88, 0x00, 0x00, 0x00, 0x3b, 0xc1, 0x74, 0x16, 0x39, 0x58, 0x70, 0x75,
            0x03, 0x89, 0x48, 0x70, 0x39, 0x78, 0x74, 0x75, 0x03, 0x89, 0x48, 0x74,
        ],
        "removed-object sweep: walks [actor+0x88], clears +0x70 / +0x74 when they name the removed object",
    ),
    (
        0x441780,
        &[
            0x56, 0x8b, 0x74, 0x24, 0x08, 0x57, 0x8d, 0x46, 0x3c, 0x50, 0x8d, 0x4e, 0x48, 0x51,
            0x8b, 0x0d, 0xc0, 0xdc, 0x71, 0x00, 0xe8, 0xb7, 0x73, 0x07, 0x00, 0x8b, 0xf8, 0x33,
            0xc0, 0x3b, 0xf8, 0x74, 0x16, 0x8b, 0x0f, 0x8b, 0x11, 0xff, 0x12, 0x89, 0x46, 0x70,
            0x8b, 0x0f, 0x8b, 0x01, 0xff, 0x50, 0x04, 0x5f, 0x89, 0x46, 0x74, 0x5e, 0xc3, 0x5f,
            0x89, 0x46, 0x70, 0x89, 0x46, 0x74, 0x5e, 0xc3,
        ],
        "BEEPER_SEEKER callback: beeper search 0x4b8b50 on the projectile's position/heading; stores the found object's two virtual-call values at +0x70/+0x74, or zero for both when none",
    ),
    (
        0x441864,
        &[
            0x8b, 0x88, 0x10, 0x02, 0x00, 0x00, 0x8b, 0x11, 0xf6, 0xc6, 0x80,
        ],
        "projectile setup 0x441830: loads the weapon's +0x210 record, tests dh & 0x80 of its flag word (BEEPER_SEEKER) ...",
    ),
    (
        0x441877,
        &[0xc7, 0x83, 0x88, 0x06, 0x00, 0x00, 0x80, 0x17, 0x44, 0x00],
        "... installs the callback 0x441780 into the projectile slot +0x688",
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

/// Every virtual address in `.text` holding a `mov [reg+0x70], x` with a
/// non-stack register base (see [`POINTER_BASE_STORES`]).
fn pointer_base_stores(text: &[u8]) -> Vec<u64> {
    let mut found = Vec::new();
    for i in 0..text.len().saturating_sub(2) {
        let store = text[i] == 0x89
            && (0x40..=0x7f).contains(&text[i + 1])
            && text[i + 1] & 7 != 4
            && text[i + 2] == 0x70;
        let immediate = text[i] == 0xc7
            && (0x40..=0x47).contains(&text[i + 1])
            && text[i + 1] != 0x44
            && text[i + 2] == 0x70;
        if store || immediate {
            found.push(TEXT_START + i as u64);
        }
    }
    found
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

/// The code half: the owner-supplied image carries the byte windows every
/// measured fact rests on, and the inventory of stores is closed.
#[test]
#[ignore = "requires CS_ENGINE_IMAGE"]
fn accept_t2116_image_pins_the_target_writers() {
    let image = image();
    for (address, bytes, meaning) in IMAGE_PINS {
        assert_eq!(
            at(&image, address, bytes.len()),
            bytes,
            "0x{address:x} no longer carries: {meaning}"
        );
    }

    // Closed inventory: no other store of a pointer-based `+0x70` exists.
    // Besides the two clears and the BEEPER_SEEKER callback's pair, every
    // entry was read in its context and belongs to another structure (finding,
    // "The store inventory").
    assert_eq!(
        pointer_base_stores(text(&image)),
        POINTER_BASE_STORES.to_vec(),
        "the inventory of `[reg+0x70]` stores changed"
    );
    // Inside the projectile module (0x5ab000..0x5b2000) there are exactly
    // three: the sweep's clear, a record constant and the launch zeroing.
    let in_module: Vec<u64> = POINTER_BASE_STORES
        .iter()
        .copied()
        .filter(|address| (0x5a_b000..0x5b_2000).contains(address))
        .collect();
    assert_eq!(in_module, [0x5a_c0f1, 0x5a_d95e, 0x5a_f610]);

    // The launch has six direct callers and nothing else names it.
    let callers: Vec<u64> = LAUNCH_CALLERS.iter().map(|(address, _)| *address).collect();
    assert_eq!(direct_calls(text(&image), LAUNCH), callers);
    let absolute = (LAUNCH as u32).to_le_bytes();
    assert!(
        !image.windows(4).any(|window| window == absolute),
        "the launch routine's address is stored as data: an indirect caller may exist"
    );
    for (call, after) in LAUNCH_CALLERS {
        assert_eq!(
            at(&image, call + 5, after.len()),
            after,
            "the code after the launch call at 0x{call:x} changed"
        );
    }
}

/// The data half: of the twelve guided (`LOCK_ON`) records of `weapons.zrd`,
/// only `wep_11` declares `BEEPER_SEEKER`, the one declaration the callback
/// above serves; no record outside the guided twelve declares it.
#[test]
#[ignore = "requires CS_GAME_DIR"]
fn accept_t2116_only_wep_11_declares_beeper_seeker() {
    let dir = std::env::var_os("CS_GAME_DIR")
        .expect("CS_GAME_DIR is not set: this measurement needs the original installation");
    let blasts = cs_content::ordnance::read_original_rocket_blasts(&PathBuf::from(dir))
        .expect("weapons.zrd reads");
    let lock_on: BTreeSet<&str> = blasts
        .iter()
        .filter(|blast| blast.lock_on.is_some())
        .map(|blast| blast.record.as_str())
        .collect();
    let seekers: BTreeSet<&str> = blasts
        .iter()
        .filter(|blast| blast.keys.contains("BEEPER_SEEKER"))
        .map(|blast| blast.record.as_str())
        .collect();
    assert_eq!(lock_on.len(), 12, "the twelve guided records: {lock_on:?}");
    assert_eq!(seekers, BTreeSet::from(["wep_11"]));
    assert!(seekers.is_subset(&lock_on));
}
