//! Task #548 against the owner's installation: what the original's rocket
//! records and area routine say about a lost-target blast.
//!
//! Task test prefix: `accept_t548_`. Findings:
//! `docs/findings/2026-10-11-f28-c-2-lost-target-blast-measured.md`.
//!
//! Two kinds of fact are pinned, and neither is evidence of how the original
//! *behaves* (no agent can run the original; `retail` is read access):
//!
//! * the **data**: the blast-relevant keys of every `ROCKET` record in
//!   `ZBD/zrdr.zbd`'s `weapons.zrd`, read through the production reader
//!   [`read_original_rocket_blasts`];
//! * the **code shape**: a handful of byte windows of the owner-supplied
//!   decrypted executable (`$CS_ENGINE_IMAGE`, sha-256
//!   `43540fc9…6c37d75`, base `0x400000`) that carry the rule the findings
//!   describe. A window is a short pin, not an extract: no code is printed or
//!   committed beyond the byte strings below.
//!
//! Both are `#[ignore = "requires CS_GAME_DIR"]` and fail loudly when their
//! inputs are missing.

use std::collections::BTreeSet;
use std::path::PathBuf;

use cs_content::ordnance::{OriginalRocketBlast, read_original_rocket_blasts};
use cs_sim::weapons::ordnance::AreaEffect;

/// The image base the executable's virtual addresses are relative to.
const IMAGE_BASE: u64 = 0x40_0000;

/// Keys that would change the rule below if any rocket record declared them:
/// the original's code reads them in the detonation and area routines.
const KEYS_NO_ROCKET_DECLARES: [&str; 7] = [
    "EXPIRES",
    "MINE",
    "DETONATE_AT_RANGE",
    "HIT_OWNER",
    "REMOTE_DETONATE",
    "TETHER_GUIDED",
    "MULTI_TARGET",
];

/// Every key any retail `ROCKET` record declares at its top level. A key the
/// importer has not seen would be an unmodelled blast, arming or channel
/// declaration, so the set is pinned exactly.
const MEASURED_ROCKET_KEYS: [&str; 41] = [
    "ACCELERATION",
    "AMMO_LIMIT",
    "ARMOR_DAMAGE",
    "BEEPER",
    "BEEPER_SEEKER",
    "CLUSTER_SIZE",
    "CRATER",
    "DAMAGE",
    "DAMAGES_ZEPPELIN",
    "DESC",
    "DESTROY_ANIMATION",
    "DETONATION_DISTANCE",
    "DETONATION_DOT_PRODUCT",
    "DETONATION_TIME",
    "FIRE",
    "FIRE_RATE",
    "FLASH",
    "FLYOUT",
    "FLYOUT_HEALTH",
    "GRAVITY",
    "HEALTH_DAMAGE",
    "HIGH_EXPLOSIVE",
    "IMPACT",
    "IMPACT_PROXIMITY",
    "LOCK_ON",
    "LOCK_ON_LEAD",
    "NAME",
    "PRIORITY",
    "PROJECTILE_BBOX",
    "RANGE",
    "RANGE_MINIMUM",
    "REAR",
    "ROCKET",
    "SHAKES_CAMERA",
    "SMOKE_SCREEN",
    "SONIC",
    "TANGLER",
    "TARGETABLE",
    "TORPEDO",
    "TURN_RATE",
    "VELOCITY",
];

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

/// The data half: fifteen rocket records, one blast radius and one pair of
/// damage channels each, and none of the keys that would change the rule.
#[test]
#[ignore = "requires CS_GAME_DIR"]
fn accept_t548_retail_rocket_records_declare_one_radius_and_one_channel_pair() {
    let rockets = rockets();
    assert_eq!(rockets.len(), 15, "fifteen retail records carry ROCKET");

    let union: BTreeSet<&str> = rockets
        .iter()
        .flat_map(|rocket| rocket.keys.iter().map(String::as_str))
        .collect();
    let expected: BTreeSet<&str> = MEASURED_ROCKET_KEYS.into_iter().collect();
    assert_eq!(
        union, expected,
        "a new top-level key is an unmodelled blast, arming or channel declaration"
    );
    for key in KEYS_NO_ROCKET_DECLARES {
        assert!(
            !union.contains(key),
            "{key} would change the measured detonation rule, and no retail rocket declares it"
        );
    }
    // No record carries a separate blast channel set: damage is the armor and
    // health pair (or the single `DAMAGE` of the two flash records).
    for rocket in &rockets {
        let channels: Vec<&str> = ["ARMOR_DAMAGE", "HEALTH_DAMAGE", "DAMAGE"]
            .into_iter()
            .filter(|key| rocket.keys.contains(*key))
            .collect();
        assert!(
            channels == ["ARMOR_DAMAGE", "HEALTH_DAMAGE"]
                || channels == ["DAMAGE"]
                || channels.is_empty(),
            "{} declares {channels:?}",
            rocket.record
        );
    }

    let incendiary = record(&rockets, "wep_04");
    assert_eq!(incendiary.impact_proximity, Some(25.0));
    assert_eq!(incendiary.detonation_distance, Some(30.0));
    assert_eq!(incendiary.armor_damage, Some(25.0));
    assert_eq!(incendiary.health_damage, Some(25.0));
    assert_eq!(incendiary.lock_on, Some(3.0));
    let flak = record(&rockets, "wep_07");
    assert_eq!(flak.impact_proximity, Some(100.0));
    assert_eq!(flak.detonation_distance, Some(50.0));
    let flare = record(&rockets, "wep_15");
    assert_eq!(flare.detonation_time_s, Some(2.0));
    assert_eq!(flare.detonation_distance, None);
}

/// The runtime consumer: a retail record's radius, in the original's own
/// units, drives the shared falloff. The units are not metres (the unit scale
/// is a separate measurement); only the ratio is, so the fractions are exact.
#[test]
#[ignore = "requires CS_GAME_DIR"]
fn accept_t548_retail_radius_drives_the_measured_falloff() {
    let rockets = rockets();
    for rocket in &rockets {
        let Some(radius) = rocket.impact_proximity else {
            continue;
        };
        let area = AreaEffect::try_new(radius, 1).expect("a retail radius is a valid area");
        assert_eq!(area.blast_fraction(0.0), Some(1.0), "{}", rocket.record);
        assert_eq!(
            area.blast_fraction(radius / 2.0),
            Some(0.75),
            "{}",
            rocket.record
        );
        assert_eq!(area.blast_fraction(radius), Some(0.0), "{}", rocket.record);
    }
}

/// `(virtual address, bytes, what the window carries)`.
const IMAGE_PINS: [(u64, &[u8], &str); 5] = [
    (
        0x005a_cb37,
        &[
            0xd9, 0x45, 0x00, 0xd8, 0x73, 0x40, 0xd8, 0x2d, 0xdc, 0x32, 0x60, 0x00,
        ],
        "area routine: entry value / IMPACT_PROXIMITY^2, subtracted from 1.0 (the falloff)",
    ),
    (
        0x005a_cb1b,
        &[0x8b, 0x43, 0x74, 0xf6, 0xc4, 0x40, 0x74, 0x14],
        "area routine: the MINE flag (bit 14) skips the falloff and takes the full damage",
    ),
    (
        0x005a_c0ec,
        &[0x39, 0x58, 0x70, 0x75, 0x03, 0x89, 0x48, 0x70],
        "a removed actor clears every projectile's target pointer (offset 0x70): the lost target",
    ),
    (
        0x005b_021b,
        &[0x8b, 0x4e, 0x70, 0x85, 0xc9, 0x74, 0x78],
        "detonation test: no target pointer skips the DETONATION_DISTANCE proximity branch",
    ),
    (
        0x005b_029a,
        &[
            0xd9, 0x47, 0x48, 0xd8, 0x1d, 0xc8, 0x32, 0x60, 0x00, 0xdf, 0xe0, 0xf6, 0xc4, 0x41,
            0x75, 0x75,
        ],
        "detonation test: the DETONATION_TIME timer is the fallback when the proximity branch is skipped",
    ),
];

/// The code half: the owner-supplied image carries the byte windows the
/// findings rest on, and the constant the falloff subtracts from is 1.0.
#[test]
#[ignore = "requires CS_GAME_DIR and CS_ENGINE_IMAGE"]
fn accept_t548_image_pins_the_falloff_the_target_clear_and_the_timer_fallback() {
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
        at(0x0060_32dc, 4),
        1.0_f32.to_le_bytes(),
        "the falloff subtracts from 1.0"
    );
    assert_eq!(
        at(0x0060_32c8, 4),
        0.0_f32.to_le_bytes(),
        "the detonation tests compare against 0.0"
    );
}
