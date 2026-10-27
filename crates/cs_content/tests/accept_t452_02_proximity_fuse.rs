//! Task #2128 (T452.02): the shape, sampling and arming of the original's
//! proximity fuse.
//!
//! Task test prefix: `accept_t452_02_`. Findings:
//! `docs/findings/2026-10-25-t452-02-proximity-fuse-shape-and-arming.md`.
//!
//! The code half pins byte windows of the owner-supplied decrypted executable
//! (`$CS_ENGINE_IMAGE`, base `0x400000`): static-analysis evidence, never
//! original-run behaviour. The data half derives each guided record's fuse
//! through the production reader.

use std::path::PathBuf;

use cs_content::ordnance::{OriginalProximitySampling, read_original_rocket_blasts};

const IMAGE_BASE: u64 = 0x40_0000;

const PINS: [(u64, &[u8], &str); 5] = [
    (
        0x5b_0216,
        &[
            0xf6, 0xc4, 0x80, 0x74, 0x7f, 0x8b, 0x4e, 0x70, 0x85, 0xc9, 0x74, 0x78, 0xd9, 0x47,
            0x44, 0xd8, 0x1d, 0xc8, 0x32, 0x60, 0x00, 0xdf, 0xe0, 0xf6, 0xc4, 0x40, 0x75, 0x68,
            0x51, 0x8d, 0x5e, 0x48, 0x53, 0xe8, 0x44, 0x86, 0xf8, 0xff, 0xd8, 0x5f, 0x44, 0x83,
            0xc4, 0x08, 0xdf, 0xe0, 0xf6, 0xc4, 0x41, 0x74, 0x51, 0x8b, 0xcb,
        ],
        "below RANGE: LOCK_ON (0x8000), a held target +0x70 and DETONATION_DISTANCE squared (+0x44) > 0, \
         then squared distance from the projectile +0x48 to the target (0x538880) <= +0x44 bursts",
    ),
    (
        0x5a_f810,
        &[
            0x8d, 0x45, 0xe8, 0x50, 0x56, 0x57, 0xe8, 0x35, 0x05, 0x00, 0x00, 0x83, 0xc4, 0x0c,
        ],
        "the step calls 0x5afd50 (which holds the proximity test) with a displacement buffer",
    ),
    (
        0x5a_f854,
        &[
            0x8d, 0x46, 0x54, 0x8d, 0x4d, 0xe8, 0x8d, 0x56, 0x48, 0x89, 0x45, 0xfc, 0x89, 0x4d,
            0xf8, 0x89, 0x55, 0xf4, 0x8b, 0x5d, 0xf4, 0x8b, 0x4d, 0xf8, 0x8b, 0x55, 0xfc, 0xd9,
            0x03, 0xd8, 0x01,
        ],
        "afterwards, new position +0x54 = position +0x48 + displacement: the proximity test ran on +0x48, before the move",
    ),
    (
        0x5b_063d,
        &[
            0x8b, 0x4f, 0x74, 0x81, 0xe1, 0x00, 0x00, 0x00, 0x80, 0x74, 0x34, 0xd9, 0x47, 0x28,
            0xd8, 0x1d, 0xc8, 0x32, 0x60, 0x00, 0xdf, 0xe0, 0xf6, 0xc4, 0x40, 0x74, 0x24, 0xd9,
            0x86, 0x64, 0x06, 0x00, 0x00, 0xd8, 0x5f, 0x24,
        ],
        "the impact path (not the proximity burst) tests RANGE_MINIMUM: flag bit 31, [1] == 0, travelled +0x664 < [0] +0x24",
    ),
    (
        0x5b_0310,
        &[0xe8, 0x8b, 0xc0, 0xff, 0xff, 0x83, 0xc4, 0x08],
        "the proximity burst calls 0x5ac3a0 directly",
    ),
];

fn image() -> Vec<u8> {
    let path = std::env::var_os("CS_ENGINE_IMAGE").expect(
        "CS_ENGINE_IMAGE is not set: this measurement needs the owner-supplied decrypted \
         executable",
    );
    std::fs::read(&path).expect("the engine image reads")
}

#[test]
#[ignore = "requires CS_ENGINE_IMAGE"]
fn accept_t452_02_image_pins_the_proximity_fuse() {
    let image = image();
    // The byte windows above are all of the fuse's tests.
    for (address, bytes, meaning) in PINS {
        let offset = usize::try_from(address - IMAGE_BASE).expect("offset");
        assert_eq!(
            &image[offset..offset + bytes.len()],
            bytes,
            "0x{address:x} no longer carries: {meaning}"
        );
    }
}

#[test]
#[ignore = "requires CS_GAME_DIR"]
fn accept_t452_02_every_guided_record_has_a_sphere_fuse_except_the_flare() {
    let dir = std::env::var_os("CS_GAME_DIR")
        .expect("CS_GAME_DIR is not set: this measurement needs the original installation");
    let blasts = read_original_rocket_blasts(&PathBuf::from(dir)).expect("weapons.zrd reads");
    let fuses: Vec<_> = blasts.iter().filter_map(|b| b.proximity_fuse()).collect();
    let got: Vec<(&str, f64)> = fuses
        .iter()
        .map(|f| (f.record.as_str(), f.trigger_radius_m))
        .collect();
    assert_eq!(
        got,
        [
            ("wep_04", 30.0),
            ("wep_05", 15.0),
            ("wep_06", 15.0),
            ("wep_07", 50.0),
            ("wep_08", 35.0),
            ("wep_09", 30.0),
            ("wep_10", 15.0),
            ("wep_11", 15.0),
            ("wep_14", 1.0),
            ("wep_24", 15.0),
            ("wep_27", 20.0),
        ]
    );
    assert!(
        fuses
            .iter()
            .all(|f| f.sampling == OriginalProximitySampling::PointSamplePerStep)
    );
    // Only the torpedo declares an arming window, and its proximity fuse
    // ignores it.
    let armed: Vec<_> = fuses
        .iter()
        .filter(|f| f.range_minimum_not_tested_m.is_some())
        .map(|f| (f.record.as_str(), f.range_minimum_not_tested_m))
        .collect();
    assert_eq!(armed, [("wep_14", Some(300.0))]);
}
