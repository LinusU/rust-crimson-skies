//! Task #1318 against the owner's installation: the unknowns task #548 left
//! open about the original ordnance blast.
//!
//! Task test prefix: `accept_t1318_`. Findings:
//! `docs/findings/2026-10-18-t1318-blast-unknowns-measured.md`.
//!
//! Two kinds of fact are pinned, and neither is evidence of how the original
//! *behaves* (no agent can run the original; `retail` is read access):
//!
//! * the **data**: `RANGE_MINIMUM`'s two-value declaration as
//!   [`read_original_rocket_blasts`] now returns it — the arming pair;
//! * the **code shape**: byte windows of the owner-supplied decrypted
//!   executable (`$CS_ENGINE_IMAGE`, sha-256 `43540fc9…6c37d75`, base
//!   `0x400000`) that carry each measurement. A window is a short pin, not an
//!   extract: no code is printed or committed beyond the byte strings below.
//!
//! Both are `#[ignore = "requires CS_GAME_DIR"]` and fail loudly when their
//! inputs are missing.

use std::path::PathBuf;

use cs_content::ordnance::{OriginalRocketBlast, read_original_rocket_blasts};

/// The image base the executable's virtual addresses are relative to.
const IMAGE_BASE: u64 = 0x40_0000;

fn game_dir() -> PathBuf {
    let dir = std::env::var_os("CS_GAME_DIR").expect(
        "CS_GAME_DIR is not set: this measurement needs the original installation \
         (capability `retail`)",
    );
    let dir = PathBuf::from(dir);
    assert!(
        dir.is_dir(),
        "CS_GAME_DIR {} is not a directory",
        dir.display()
    );
    dir
}

fn rockets() -> Vec<OriginalRocketBlast> {
    read_original_rocket_blasts(&game_dir()).expect("the retail weapon records read")
}

fn record<'a>(rockets: &'a [OriginalRocketBlast], name: &str) -> &'a OriginalRocketBlast {
    rockets
        .iter()
        .find(|rocket| rocket.record == name)
        .unwrap_or_else(|| panic!("retail has no {name} rocket record"))
}

/// The data half: the torpedo record's `RANGE_MINIMUM` reads as the measured
/// arming pair — a distance in metres and the blast scale inside the window —
/// and the scale never appears without its distance.
#[test]
#[ignore = "requires CS_GAME_DIR"]
fn accept_t1318_retail_range_minimum_reads_the_arming_pair() {
    let rockets = rockets();
    let torpedo = record(&rockets, "wep_14");
    assert!(
        torpedo.keys.contains("TORPEDO"),
        "wep_14 is the torpedo record"
    );
    assert_eq!(torpedo.range_minimum, Some(300.0));
    assert_eq!(torpedo.range_minimum_blast_scale, Some(0.0));
    for rocket in &rockets {
        if rocket.range_minimum_blast_scale.is_some() {
            assert!(
                rocket.range_minimum.is_some(),
                "{} declares a blast scale with no arming distance",
                rocket.record
            );
        }
    }
}

/// `(virtual address, bytes, what the window carries)`.
const IMAGE_PINS: [(u64, &[u8], &str); 30] = [
    (
        0x005a_dd70,
        &[
            0xd9, 0x42, 0x0c, 0xd9, 0xc0, 0xd8, 0xc9, 0xd9, 0x5b, 0x40, 0xdd, 0xd8,
        ],
        "IMPACT_PROXIMITY parse: the record value squared into weapon +0x40 with no unit conversion (the value is metres)",
    ),
    (
        0x005a_dbb2,
        &[0xd9, 0x40, 0x0c, 0xd9, 0xc0, 0xd8, 0xc9, 0xd9, 0x5b, 0x44],
        "DETONATION_DISTANCE parse: squared into +0x44 with no unit conversion",
    ),
    (
        0x005a_dfcd,
        &[
            0x81, 0x4b, 0x74, 0x00, 0x00, 0x00, 0x80, 0x8b, 0x48, 0x04, 0x8b, 0x51, 0x0c, 0x89,
            0x53, 0x24, 0x8b, 0x40, 0x04, 0x8b, 0x48, 0x14, 0x89, 0x4b,
        ],
        "RANGE_MINIMUM parse: flag bit 31 set, [0] stored at weapon +0x24, [1] at +0x28",
    ),
    (
        0x005a_f092,
        &[
            0x8b, 0x15, 0xb0, 0xe1, 0xa1, 0x00, 0x89, 0x97, 0x78, 0x06, 0x00, 0x00,
        ],
        "projectile spawn: blast scale +0x678 initialised from global 0xa1e1b0",
    ),
    (
        0x005a_f0c3,
        &[0xc7, 0x05, 0xb0, 0xe1, 0xa1, 0x00, 0x00, 0x00, 0x80, 0x3f],
        "global 0xa1e1b0 reset to 1.0f after the spawn copy (every projectile starts at scale 1.0)",
    ),
    (
        0x005b_06c6,
        &[
            0xd9, 0x47, 0x28, 0xd8, 0x8e, 0x78, 0x06, 0x00, 0x00, 0xd9, 0x9e, 0x78, 0x06, 0x00,
        ],
        "step: a detonation inside the arming window multiplies projectile +0x678 by weapon +0x28 (RANGE_MINIMUM[1])",
    ),
    (
        0x005a_cb0f,
        &[
            0x39, 0x82, 0xbc, 0x00, 0x00, 0x00, 0x0f, 0x84, 0x1c, 0x01, 0x00, 0x00,
        ],
        "apply loop: a recipient whose +0xbc equals the exclusion key is skipped",
    ),
    (
        0x005a_cb56,
        &[
            0xd8, 0x8e, 0x78, 0x06, 0x00, 0x00, 0xd9, 0x5c, 0x24, 0x14, 0xd8, 0x8e,
        ],
        "apply loop: both stored damage channels are multiplied by projectile +0x678",
    ),
    (
        0x005b_0741,
        &[
            0x8b, 0x4b, 0x24, 0x8b, 0x91, 0xbc, 0x00, 0x00, 0x00, 0x52, 0x8d,
        ],
        "impact path: the exclusion key is [hit object +0x24]->+0xbc (detonations pass 0)",
    ),
    (
        0x005a_c415,
        &[0xa1, 0xd4, 0xd7, 0xa1, 0x00, 0x83, 0xc4, 0x10],
        "detonation routine: the apply call is gated on global 0xa1d7d4",
    ),
    (
        0x005b_09f8,
        &[0xc7, 0x05, 0xd4, 0xd7, 0xa1, 0x00, 0x00, 0x00, 0x00, 0x00],
        "nested impact processing clears 0xa1d7d4 (the re-entrancy guard)",
    ),
    (
        0x005b_0a0b,
        &[0xc7, 0x05, 0xd4, 0xd7, 0xa1, 0x00, 0x01, 0x00, 0x00, 0x00],
        "nested impact processing restores 0xa1d7d4 to 1",
    ),
    (
        0x004c_b70d,
        &[
            0xd8, 0x64, 0x24, 0x58, 0x83, 0xc4, 0x08, 0xd8, 0x15, 0xc8, 0x32, 0x60, 0x00, 0xdf,
        ],
        "area query: centre distance minus the recipient bound, compared against 0.0",
    ),
    (
        0x004c_b72d,
        &[0xd9, 0xc0, 0xd8, 0xc9, 0xd9, 0x5c, 0x24, 0x10],
        "area query: the clamped surface distance squared — the entry value",
    ),
    (
        0x004c_b73b,
        &[0xd8, 0x1d, 0x98, 0xe8, 0x71, 0x00],
        "area query: the squared entry value compared against radius^2 at 0x71e898",
    ),
    (
        0x004c_b80a,
        &[0x89, 0x50, 0x1c],
        "area query: the squared surface distance stored at entry +0x1c",
    ),
    (
        0x004c_d245,
        &[
            0x83, 0xc9, 0x10, 0x89, 0x48, 0x24, 0x33, 0xc0, 0xc3, 0x83, 0xe1, 0xef,
        ],
        "0x4cd210: sets or clears node flag bit 0x10 at +0x24 (the RANGE_MINIMUM call)",
    ),
    (
        0x005b_0318,
        &[0xc7, 0x46, 0x20, 0x00, 0x00, 0x00, 0x00],
        "step past RANGE: the projectile's +0x20 active word is cleared",
    ),
    (
        0x005a_f917,
        &[0x8b, 0x46, 0x20, 0x85, 0xc0, 0x74, 0x11],
        "projectile sweep: +0x20 == 0 unlinks the projectile and frees it via 0x5b08d0",
    ),
    (
        0x004b_a738,
        &[0x81, 0x0e, 0x00, 0x02, 0x00, 0x00],
        "special-key parser: SONIC sets flag bit 0x200",
    ),
    (
        0x004b_a750,
        &[0x81, 0x0e, 0x00, 0x08, 0x00, 0x00],
        "special-key parser: FLASH sets flag bit 0x800",
    ),
    (
        0x004b_a76b,
        &[0x81, 0xc9, 0x00, 0x40, 0x00, 0x00],
        "special-key parser: BEEPER sets flag bit 0x4000",
    ),
    (
        0x004b_a779,
        &[0xc7, 0x46, 0x18, 0x00, 0x00, 0xa0, 0x40],
        "BEEPER's optional TIME parameter defaults to 5.0f at record +0x18",
    ),
    (
        0x004b_a7c5,
        &[0x81, 0x0e, 0x00, 0x80, 0x00, 0x00],
        "special-key parser: BEEPER_SEEKER sets flag bit 0x8000",
    ),
    (
        0x004b_a7f8,
        &[0x81, 0xc9, 0x00, 0x00, 0x01, 0x00],
        "special-key parser: SMOKE_SCREEN sets flag bit 0x10000",
    ),
    (
        0x004b_a806,
        &[0xc7, 0x46, 0x18, 0x00, 0x00, 0xa0, 0x40],
        "SMOKE_SCREEN's optional TIME parameter defaults to 5.0f at record +0x18",
    ),
    (
        0x004b_a859,
        &[0x81, 0x0e, 0x00, 0x00, 0x04, 0x00],
        "special-key parser: TANGLER sets flag bit 0x40000",
    ),
    (
        0x005a_ec98,
        &[0x89, 0x81, 0x0c, 0x02, 0x00, 0x00],
        "TANGLER: the parser registers a tick callback at weapon +0x20c (0x4ba660)",
    ),
    (
        0x005a_f98b,
        &[0xa9, 0x00, 0x00, 0x01, 0x00],
        "projectile step: the SMOKE_SCREEN bit gates the after-TIME emitter call",
    ),
    (
        0x005a_c850,
        &[0xa9, 0x00, 0x00, 0x04, 0x00],
        "impact path: the TANGLER bit gates the status-application call (0x5ac580)",
    ),
];

/// The code half: the owner-supplied image carries the byte windows every
/// measured fact rests on.
#[test]
#[ignore = "requires CS_GAME_DIR and CS_ENGINE_IMAGE"]
fn accept_t1318_image_pins_the_blast_unknowns() {
    let path = std::env::var_os("CS_ENGINE_IMAGE").expect(
        "CS_ENGINE_IMAGE is not set: this measurement needs the owner-supplied decrypted \
         executable",
    );
    let image = std::fs::read(&path).expect("the engine image reads");
    let at = |address: u64, length: usize| -> &[u8] {
        let offset = usize::try_from(address - IMAGE_BASE).expect("offset");
        &image[offset..offset + length]
    };
    for (address, bytes, meaning) in IMAGE_PINS {
        assert_eq!(
            at(address, bytes.len()),
            bytes,
            "0x{address:x} no longer carries: {meaning}"
        );
    }
    assert_eq!(
        at(0x0060_32c8, 4),
        0.0_f32.to_le_bytes(),
        "the surface distance is clamped against 0.0"
    );
}
