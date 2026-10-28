//! Task T452-R.01 (#2159, split of #2156): measure the original ordnance
//! launch geometry — the `VELOCITY` unit and application, launcher-velocity
//! inheritance, `FIRE_RATE` semantics, the `RANGE`-end lifetime behaviour and
//! parser defaults, and the mount/slot surface.
//!
//! Task test prefix: `accept_t452_r01_`. Findings:
//! `docs/findings/2026-10-28-t452-r01-ordnance-launch-geometry.md`.
//!
//! The pins are byte windows and byte-level inventories of the owner-supplied
//! decrypted executable (`$CS_ENGINE_IMAGE`, base `0x400000`): static-analysis
//! evidence, never original-run behaviour, and no code is printed or committed
//! beyond the byte strings below. The synthetic half drives the production
//! `original_launch_geometry` / `original_component_table` readers and runs in
//! CI; the retail half re-reads the installation and the image and fails
//! loudly without `CS_GAME_DIR` / `CS_ENGINE_IMAGE`.

use std::collections::{BTreeMap, BTreeSet};

use cs_content::ordnance::{
    ORIGINAL_RANGE_DEFAULT_M, OriginalInheritance, OriginalLaunchSpeed, OriginalMountDeclaration,
    OriginalRangeBound, OriginalWeaponRecord, T452_04_MOUNT, T452_04_RELEASE_DELAY,
    T452_04_VELOCITY_UNIT, classify_original_weapons, original_component_table,
    original_launch_geometry, original_launch_geometry_table, read_original_component_table,
    read_original_weapon_census,
};

/// The image base the executable's virtual addresses are relative to.
const IMAGE_BASE: u64 = 0x40_0000;
/// First byte of `.text` (file offset equals RVA there) and its length.
const TEXT_START: u64 = 0x40_1000;
const TEXT_LEN: usize = 0x20_14bd;

/// Byte windows every measured fact rests on: address, bytes, what they are.
const IMAGE_PINS: [(u64, &[u8], &str); 21] = [
    (
        0x5a_d80b,
        &[0xc7, 0x43, 0x1c, 0x00, 0x00, 0xfa, 0x43],
        "parser default: record +0x1c (RANGE) = 500.0f before any key is read",
    ),
    (
        0x5a_d812,
        &[0xc7, 0x43, 0x2c, 0x00, 0x00, 0xf0, 0x42],
        "parser default: record +0x2c (VELOCITY) = 120.0f before any key is read",
    ),
    (
        0x5a_d82a,
        &[0xc7, 0x43, 0x48, 0x00, 0x00, 0x80, 0xbf],
        "parser default: record +0x48 (DETONATION_TIME) = -1.0f, i.e. no timed detonation",
    ),
    (
        0x5a_d7bf,
        &[0x57, 0xff, 0x15, 0xb4, 0x01, 0xa2, 0x00],
        "the record buffer comes from `alloc(size, 1)` at 0x5ad7bf; the +0x74 flag word's read-modify-write stores prove the buffer starts zeroed, so ACCELERATION (+0x38) defaults to 0",
    ),
    (
        0x5a_dcc9,
        &[0xd9, 0x05, 0xdc, 0x32, 0x60, 0x00, 0xd8, 0x72, 0x0c],
        "FIRE_RATE: `fld 1.0; fdiv [value]` — the parser stores the reciprocal",
    ),
    (
        0x5a_dcd2,
        &[0xd9, 0x5b, 0x14],
        "FIRE_RATE: `fstp [record+0x14]` — +0x14 = 1.0/FIRE_RATE, an interval in seconds",
    ),
    (
        0x5a_e0ac,
        &[
            0xf6, 0x43, 0x74, 0x02, 0x75, 0x09, 0x8b, 0x40, 0x04, 0x8b, 0x48, 0x0c,
        ],
        "VELOCITY store: skipped when flag bit 0x2 of +0x74 (set by BEAM) is on",
    ),
    (
        0x5a_e0b8,
        &[0x89, 0x4b, 0x2c],
        "VELOCITY: the declared value lands in record +0x2c",
    ),
    (
        0x5a_dfb5,
        &[0x89, 0x43, 0x1c, 0xd9, 0x5b, 0x20],
        "RANGE: value to +0x1c, its square to +0x20",
    ),
    (
        0x5a_dfcd,
        &[
            0x81, 0x4b, 0x74, 0x00, 0x00, 0x00, 0x80, 0x8b, 0x48, 0x04, 0x8b, 0x51, 0x0c, 0x89,
            0x53, 0x24,
        ],
        "RANGE_MINIMUM: sets flag bit 0x80000000 of +0x74 and stores the value at +0x24",
    ),
    (
        0x5a_dbaf,
        &[
            0x8b, 0x40, 0x04, 0xd9, 0x40, 0x0c, 0xd9, 0xc0, 0xd8, 0xc9, 0xd9, 0x5b, 0x44,
        ],
        "DETONATION_DISTANCE: value squared into +0x44",
    ),
    (
        0x5a_dbd0,
        &[0x8b, 0x48, 0x04, 0x8b, 0x51, 0x0c, 0x89, 0x53, 0x48],
        "DETONATION_TIME: the declared seconds land in +0x48",
    ),
    (
        0x5a_e4df,
        &[
            0xd9, 0x43, 0x54, 0xd8, 0x1d, 0xc8, 0x32, 0x60, 0x00, 0xdf, 0xe0, 0xf6, 0xc4, 0x40,
            0x75,
        ],
        "GRAVITY post-parse: when +0x54 != 0 the parser rewrites +0x2c",
    ),
    (
        0x5a_e4ef,
        &[
            0xd9, 0x43, 0x2c, 0xd9, 0xc0, 0xd8, 0xc9, 0xd9, 0x43, 0x54, 0xdc, 0xc0, 0xde, 0xf9,
            0xd9, 0x9b, 0x00, 0x02, 0x00, 0x00, 0xdd,
        ],
        "ballistic: +0x200 = VELOCITY^2 / (2*GRAVITY), the declared speed's max climb height",
    ),
    (
        0x5a_e505,
        &[
            0xd9, 0x43, 0x54, 0xd8, 0x4b, 0x1c, 0xdc, 0xc0, 0xd9, 0x5d, 0xe4, 0x8b, 0x45, 0xe4,
            0xd1, 0xf8, 0x05, 0x00, 0x00, 0xc0, 0x1f, 0x89, 0x45, 0xe0, 0xd9, 0x45, 0xe0, 0xd9,
            0x5b, 0x2c,
        ],
        "ballistic: +0x2c = sqrt(2*GRAVITY*RANGE) via the bit-level sqrt (sar+0x1fc00000), overwriting declared VELOCITY",
    ),
    (
        0x5a_f0e7,
        &[0x8b, 0x56, 0x2c, 0x89, 0x97, 0x44, 0x06, 0x00, 0x00],
        "spawn: node +0x644 (target speed) = record +0x2c",
    ),
    (
        0x5a_f124,
        &[
            0xd9, 0x46, 0x38, 0xd8, 0x1d, 0xc8, 0x32, 0x60, 0x00, 0xdf, 0xe0, 0xf6, 0xc4, 0x40,
            0x0f, 0x84, 0xdd, 0x01, 0x00, 0x00, 0x8b, 0x46, 0x74, 0xf6, 0xc4, 0x08, 0x0f, 0x85,
            0xd1, 0x01,
        ],
        "spawn gate: the acceleration path runs only when +0x38 (ACCELERATION) != 0 and flag bit 0x800 (INSTANT) is clear",
    ),
    (
        0x5a_f144,
        &[
            0x8b, 0x4e, 0x2c, 0x89, 0x8f, 0x40, 0x06, 0x00, 0x00, 0x8b, 0x46, 0x2c, 0x8b, 0x4d,
            0x1c, 0x8d, 0x57, 0x60, 0x52, 0x50, 0x53, 0x51, 0xe8,
        ],
        "accelerating launch: node +0x640 = VELOCITY, then 0x5389a0 computes +0x60 = shooter_vel + dir*VELOCITY (full inheritance)",
    ),
    (
        0x5a_f315,
        &[
            0x8b, 0x55, 0x1c, 0xc7, 0x87, 0x40, 0x06, 0x00, 0x00, 0x17, 0xb7, 0xd1, 0x38, 0x8b,
            0x0a, 0x8d, 0x47, 0x60, 0x89, 0x08, 0x8b, 0x4a, 0x04, 0x89, 0x48, 0x04, 0x8b, 0x52,
            0x08,
        ],
        "non-accelerating/INSTANT launch: +0x640 = 1e-4, node +0x60 = shooter velocity (before the direction term is subtracted)",
    ),
    (
        0x5a_f3a3,
        &[
            0xd9, 0x03, 0xd8, 0x21, 0xd9, 0x43, 0x04, 0xd8, 0x61, 0x04, 0xd9, 0x43, 0x08, 0xd8,
            0x61, 0x08, 0xd9, 0xca, 0xd9, 0x1a, 0xd9, 0x5a, 0x04, 0xd9,
        ],
        "the else path subtracts dir*|shooter| from +0x60: the launch-direction part of the shooter velocity is removed, not added",
    ),
    (
        0x5a_fd70,
        &[
            0xd9, 0x86, 0x40, 0x06, 0x00, 0x00, 0xd8, 0x9e, 0x44, 0x06, 0x00, 0x00, 0xdf, 0xe0,
            0xf6, 0xc4, 0x01, 0x74, 0x4d, 0xd9, 0x05, 0x44, 0xd7, 0x9a, 0x00, 0xd8, 0x4f, 0x38,
        ],
        "step: while +0x640 < +0x644 the speed grows by dt*ACCELERATION (+0x38), clamped to +0x644",
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

// ---------------------------------------------------------- synthetic ----

fn record(name: &str, keys: &[&str], first: &[(&str, f64)]) -> OriginalWeaponRecord {
    OriginalWeaponRecord {
        record: name.to_owned(),
        keys: keys.iter().map(|key| (*key).to_owned()).collect(),
        first_values: first
            .iter()
            .map(|(key, value)| ((*key).to_owned(), *value))
            .collect(),
        second_values: BTreeMap::new(),
        first_texts: BTreeMap::new(),
    }
}

fn layout() -> cs_content::ordnance::OriginalMountLayout {
    const HARDPOINTS: &str = "main\n{\ngui_create\n{\nobject DT[2]\nfor (Q = 0; Q < 2; Q++)\n{\nDT[Q] = @ctl@ZQ\n}\n}\n}\n";
    const ORDNANCE_LAYOUT: &str = "main\n{\ngui_create\n{\nobject QKA[4]\nfor (K = 0; K < 4; K++)\n{\nQKA[K] = @ctl@ZQ\n}\nobject RKA[8]\nfor (K = 0; K < 8; K++)\n{\nRKA[K] = @ctl@ZQ\n}\n}\ngui_init\n{\nfor (K = 0; K < 8; K++)\n{\nif (-1 == SLOT)\n{\ndeactivate(RKA[K])\n}\n}\nfor (K = 0; K < 4; K++)\n{\nif (-1 == SLOT)\n{\ndeactivate(QKA[K])\n}\n}\n}\n}\n";
    cs_content::ordnance::OriginalMountLayout::from_screens(HARDPOINTS, ORDNANCE_LAYOUT)
        .expect("the fixture layout reads")
}

#[test]
fn accept_t452_r01_declared_velocity_flies_verbatim() {
    let geometry = original_launch_geometry(&record(
        "wep_90",
        &["ROCKET", "VELOCITY", "ACCELERATION", "RANGE"],
        &[
            ("VELOCITY", 450.0),
            ("ACCELERATION", 150.0),
            ("RANGE", 1500.0),
        ],
    ));
    assert_eq!(
        geometry.speed,
        OriginalLaunchSpeed::Declared { mps: 450.0 },
        "a declared VELOCITY with no GRAVITY and no BEAM is stored verbatim"
    );
    assert_eq!(geometry.inheritance, OriginalInheritance::Full);
    assert_eq!(geometry.acceleration_mps2, Some(150.0));
    assert_eq!(geometry.range, OriginalRangeBound::Declared { m: 1500.0 });
}

#[test]
fn accept_t452_r01_absent_velocity_and_range_take_the_parser_defaults() {
    // wep_13's shape: ROCKET with no VELOCITY, no RANGE, no ACCELERATION.
    let geometry = original_launch_geometry(&record(
        "wep_91",
        &["ROCKET", "FIRE_RATE", "CLUSTER_SIZE"],
        &[("FIRE_RATE", 4.0), ("CLUSTER_SIZE", 1.0)],
    ));
    assert_eq!(
        geometry.speed,
        OriginalLaunchSpeed::Defaulted,
        "with no VELOCITY key the parser's 120.0 stands"
    );
    assert_eq!(
        geometry.range,
        OriginalRangeBound::Defaulted,
        "with no RANGE key the parser's 500.0 stands"
    );
    assert_eq!(geometry.range.metres(), ORIGINAL_RANGE_DEFAULT_M);
    assert_eq!(geometry.acceleration_mps2, None);
    assert_eq!(
        geometry.inheritance,
        OriginalInheritance::LateralRemainder,
        "no ACCELERATION selects the else spawn path: shooter - dir*|shooter|"
    );
    assert_eq!(geometry.refire_interval_s, Some(0.25));
}

#[test]
fn accept_t452_r01_gravity_overwrites_declared_velocity() {
    // wep_25/26/27/28's shape: GRAVITY present, so +0x2c becomes
    // sqrt(2*GRAVITY*RANGE) regardless of the declared VELOCITY.
    let geometry = original_launch_geometry(&record(
        "wep_92",
        &["ROCKET", "GRAVITY", "RANGE", "VELOCITY", "ACCELERATION"],
        &[
            ("GRAVITY", 9.8),
            ("RANGE", 1000.0),
            ("VELOCITY", 450.0),
            ("ACCELERATION", 150.0),
        ],
    ));
    let expected = (2.0 * 9.8 * 1000.0_f64).sqrt();
    assert_eq!(
        geometry.speed,
        OriginalLaunchSpeed::Ballistic {
            mps: expected,
            gravity_mps2: 9.8,
            range_m: 1000.0,
        },
        "GRAVITY records fly at the ballistic speed, not the declared VELOCITY"
    );
}

#[test]
fn accept_t452_r01_ballistic_speed_uses_the_default_range_when_absent() {
    let geometry = original_launch_geometry(&record(
        "wep_93",
        &["ROCKET", "GRAVITY"],
        &[("GRAVITY", 9.8)],
    ));
    match geometry.speed {
        OriginalLaunchSpeed::Ballistic { mps, range_m, .. } => {
            assert_eq!(range_m, ORIGINAL_RANGE_DEFAULT_M);
            assert_eq!(mps, (2.0 * 9.8 * ORIGINAL_RANGE_DEFAULT_M).sqrt());
        }
        speed => panic!("GRAVITY without RANGE still ballistics: {speed:?}"),
    }
}

#[test]
fn accept_t452_r01_beam_records_take_the_reciprocal_rate() {
    let timed = original_launch_geometry(&record(
        "wep_94",
        &["ROCKET", "BEAM", "TIME_TO_MAX_RANGE", "VELOCITY"],
        &[("TIME_TO_MAX_RANGE", 2.0), ("VELOCITY", 450.0)],
    ));
    assert_eq!(
        timed.speed,
        OriginalLaunchSpeed::Beam { per_second: 0.5 },
        "TIME_TO_MAX_RANGE reciprocals into +0x2c for a BEAM record"
    );
    let untimed = original_launch_geometry(&record("wep_95", &["ROCKET", "BEAM"], &[]));
    assert_eq!(
        untimed.speed,
        OriginalLaunchSpeed::Beam { per_second: 1.0e-4 },
        "with no TIME_TO_MAX_RANGE the reciprocal of the 10000.0 default is stored"
    );
}

#[test]
fn accept_t452_r01_gravity_overrides_beam_too() {
    // The GRAVITY block is the post-parse tail (0x5ae4df): it runs after the
    // whole key loop, so a declared non-zero GRAVITY overwrites even a BEAM
    // record's rate — the parser order is BEAM, VELOCITY, then GRAVITY last.
    let geometry = original_launch_geometry(&record(
        "wep_98",
        &["ROCKET", "BEAM", "GRAVITY", "RANGE", "TIME_TO_MAX_RANGE"],
        &[
            ("GRAVITY", 9.8),
            ("RANGE", 1000.0),
            ("TIME_TO_MAX_RANGE", 2.0),
        ],
    ));
    let expected = (2.0 * 9.8 * 1000.0_f64).sqrt();
    assert_eq!(
        geometry.speed,
        OriginalLaunchSpeed::Ballistic {
            mps: expected,
            gravity_mps2: 9.8,
            range_m: 1000.0,
        },
        "the post-parse GRAVITY overwrite wins over the BEAM rate"
    );
}

#[test]
fn accept_t452_r01_instant_records_have_no_ballistic_velocity() {
    let geometry = original_launch_geometry(&record(
        "wep_96",
        &["ROCKET", "INSTANT", "ACCELERATION", "VELOCITY"],
        &[("ACCELERATION", 150.0), ("VELOCITY", 450.0)],
    ));
    assert_eq!(
        geometry.inheritance,
        OriginalInheritance::Instant,
        "INSTANT forces the else spawn path; the step displaces by RANGE*dir"
    );
}

#[test]
fn accept_t452_r01_component_table_drops_only_the_measured_claims() {
    let census = classify_original_weapons(vec![record(
        "wep_97",
        &[
            "ROCKET",
            "VELOCITY",
            "FIRE_RATE",
            "ACCELERATION",
            "DETONATION_DISTANCE",
            "RANGE",
            "CLUSTER_SIZE",
            "LOCK_ON",
        ],
        &[
            ("VELOCITY", 450.0),
            ("FIRE_RATE", 2.0),
            ("ACCELERATION", 150.0),
            ("DETONATION_DISTANCE", 20.0),
            ("RANGE", 1500.0),
            ("CLUSTER_SIZE", 1.0),
            ("LOCK_ON", 1.0),
        ],
    )]);
    let table = original_component_table(&census, &layout());
    let row = table.row("wep_97").expect("the record classified");
    // VELOCITY's unit is measured now: the claim is gone from the row.
    assert!(!row.names_claim(T452_04_VELOCITY_UNIT));
    // FIRE_RATE's mount binding is not pinned: the claim stays.
    assert!(row.names_claim(T452_04_RELEASE_DELAY));
    // The launch geometry of the same record comes from the same census.
    let geometry = original_launch_geometry_table(&census);
    assert_eq!(
        geometry["wep_97"].speed,
        OriginalLaunchSpeed::Declared { mps: 450.0 }
    );
    assert_eq!(geometry["wep_97"].refire_interval_s, Some(0.5));
}

/// The retail half: every measured value re-derived from the installation
/// and compared with the production readers.
#[test]
#[ignore = "requires CS_GAME_DIR"]
fn accept_t452_r01_retail_launch_geometry() {
    let install_root = std::env::var_os("CS_GAME_DIR")
        .map(std::path::PathBuf::from)
        .expect("CS_GAME_DIR is not set: this test needs the original installation");
    let census =
        read_original_weapon_census(&install_root).expect("the installation's weapons.zrd reads");
    let geometry = original_launch_geometry_table(&census);

    // wep_13: ROCKET, no VELOCITY, no RANGE, no ACCELERATION -> 120/500 defaults
    // and the lateral-remainder spawn path.
    let wep13 = &geometry["wep_13"];
    assert_eq!(wep13.speed, OriginalLaunchSpeed::Defaulted);
    assert_eq!(wep13.range, OriginalRangeBound::Defaulted);
    assert_eq!(wep13.range.metres(), 500.0);
    assert_eq!(wep13.acceleration_mps2, None);
    assert_eq!(wep13.inheritance, OriginalInheritance::LateralRemainder);
    assert_eq!(wep13.refire_interval_s, Some(1.0));

    // wep_15: VELOCITY 1, DETONATION_TIME 2, no RANGE, ACCELERATION 0 ->
    // declared speed 1.0, default range, lateral-remainder spawn (a declared
    // ACCELERATION of zero takes the same branch as an absent one).
    let wep15 = &geometry["wep_15"];
    assert_eq!(wep15.speed, OriginalLaunchSpeed::Declared { mps: 1.0 });
    assert_eq!(wep15.range, OriginalRangeBound::Defaulted);
    assert_eq!(wep15.acceleration_mps2, Some(0.0));
    assert_eq!(wep15.inheritance, OriginalInheritance::LateralRemainder);

    // Every record that declares GRAVITY declares GRAVITY 0, so the ballistic
    // overwrite never runs on retail data: the declared VELOCITY stands and
    // the step's gravity pull is zero. wep_04, wep_25, wep_26, wep_27
    // declare ACCELERATION 150 and take the full-inheritance spawn; wep_28
    // declares ACCELERATION 0 and takes the lateral-remainder path.
    for (name, mps, range_m, inheritance) in [
        ("wep_04", 450.0, 900.0, OriginalInheritance::Full),
        ("wep_25", 450.0, 900.0, OriginalInheritance::Full),
        ("wep_26", 450.0, 900.0, OriginalInheritance::Full),
        ("wep_27", 850.0, 900.0, OriginalInheritance::Full),
        (
            "wep_28",
            350.0,
            3500.0,
            OriginalInheritance::LateralRemainder,
        ),
    ] {
        let row = &geometry[name];
        assert_eq!(row.speed, OriginalLaunchSpeed::Declared { mps }, "{name}");
        assert_eq!(
            row.range,
            OriginalRangeBound::Declared { m: range_m },
            "{name}"
        );
        assert_eq!(row.inheritance, inheritance, "{name}");
    }

    // The full-inheritance census: the four ACCELERATION 150 records above,
    // every other record on the lateral-remainder path, no INSTANT or BEAM
    // record anywhere in the retail table.
    let full: BTreeSet<&str> = geometry
        .iter()
        .filter(|(_, row)| row.inheritance == OriginalInheritance::Full)
        .map(|(name, _)| name.as_str())
        .collect();
    assert_eq!(
        full,
        BTreeSet::from(["wep_04", "wep_25", "wep_26", "wep_27"])
    );
    assert!(
        geometry.values().all(|row| !matches!(
            row.speed,
            OriginalLaunchSpeed::Beam { .. } | OriginalLaunchSpeed::Ballistic { .. }
        )),
        "no retail record reaches the beam or ballistic arm"
    );

    // Every record's stored interval is 1/FIRE_RATE; every record declares
    // FIRE_RATE, so no row may have a missing interval.
    for record in &census.records {
        let fire_rate = record
            .first("FIRE_RATE")
            .expect("every record declares FIRE_RATE");
        let interval = geometry[&record.record]
            .refire_interval_s
            .expect("the interval is stored when FIRE_RATE is");
        assert!(
            (interval - 1.0 / fire_rate).abs() < 1e-9,
            "{}: {interval} != 1/{fire_rate}",
            record.record
        );
    }

    // The component table agrees: the measured claims moved, nothing else.
    let table = read_original_component_table(&install_root).expect("the component table reads");
    assert!(!table.claims().contains(T452_04_VELOCITY_UNIT));
    assert!(table.claims().contains(T452_04_RELEASE_DELAY));
    assert!(table.claims().contains(T452_04_MOUNT));
    let wep13_row = table.row("wep_13").expect("wep_13 is a component");
    assert_eq!(
        wep13_row.mount,
        OriginalMountDeclaration::RocketSlot { slots: 8 }
    );
    let wep25_row = table.row("wep_25").expect("wep_25 is a component");
    assert!(matches!(
        wep25_row.mount,
        OriginalMountDeclaration::Unknown { .. }
    ));
}

// ------------------------------------------------- the cited static rows ---

const TABLE: &str = include_str!("../../../data/owner-static/engine-facts.toml");

/// `(name, va, bytes)` of every `launch.*` row of `[[ordnance_launch_fact]]`.
fn launch_rows() -> Vec<(String, u64, Vec<u8>)> {
    let field = |chunk: &str, name: &str| {
        chunk
            .lines()
            .find_map(|line| {
                let rest = line.strip_prefix(name)?.trim_start().strip_prefix('=')?;
                Some(rest.trim().trim_matches('"').to_string())
            })
            .unwrap_or_else(|| panic!("an ordnance_launch_fact row lacks {name}"))
    };
    TABLE
        .split("[[ordnance_launch_fact]]")
        .skip(1)
        .map(|chunk| chunk.split("\n[").next().unwrap_or(chunk))
        .filter(|chunk| field(chunk, "name").starts_with("launch."))
        .map(|chunk| {
            let va = field(chunk, "va");
            let va = u64::from_str_radix(va.trim_start_matches("0x"), 16).expect("hex va");
            let hex = field(chunk, "bytes");
            let bytes = (0..hex.len() / 2)
                .map(|i| u8::from_str_radix(&hex[2 * i..2 * i + 2], 16).expect("hex byte"))
                .collect();
            (field(chunk, "name"), va, bytes)
        })
        .collect()
}

#[test]
fn accept_t452_r01_the_committed_rows_cite_every_measured_site() {
    let rows = launch_rows();
    let names: Vec<&str> = rows.iter().map(|(name, _, _)| name.as_str()).collect();
    assert_eq!(
        names,
        [
            "launch.range_default",
            "launch.velocity_default",
            "launch.detonation_time_default",
            "launch.fire_rate_reciprocal",
            "launch.velocity_store_beam_gate",
            "launch.beam_flags",
            "launch.beam_speed_default",
            "launch.beam_speed_gate",
            "launch.beam_speed_reciprocal",
            "launch.gravity_gate",
            "launch.ballistic_climb",
            "launch.ballistic_speed",
            "launch.spawn_target_speed",
            "launch.accel_instant_gate",
            "launch.inheritance_full",
            "launch.inheritance_mac_call",
            "launch.inheritance_remainder_head",
            "launch.inheritance_remainder_sub",
            "launch.accel_step",
            "launch.dt_displacement",
            "launch.refire_interval",
            "launch.refire_stagger",
            "launch.mount_find_stride",
            "launch.range_end",
            "launch.state_clear",
            "launch.range_cmp",
        ]
    );
    // Every committed byte window is a prefix of (or equal to) the pin that
    // carries the same fact: the table cannot cite an unpinned measurement.
    let get = |name: &str| rows.iter().find(|(n, _, _)| n == name).expect("a row");
    assert_eq!(
        &get("launch.velocity_default").2[3..],
        [0x00, 0x00, 0xf0, 0x42],
        "120.0f"
    );
    assert_eq!(
        &get("launch.range_default").2[3..],
        [0x00, 0x00, 0xfa, 0x43],
        "500.0f"
    );
    assert_eq!(
        &get("launch.detonation_time_default").2[3..],
        [0x00, 0x00, 0x80, 0xbf],
        "-1.0f"
    );
    assert_eq!(get("launch.refire_interval").2[6..9], [0xd8, 0x47, 0x14]);
    assert_eq!(
        get("launch.inheritance_mac_call").2,
        [0xe8, 0x41, 0x98, 0xf8, 0xff]
    );
    assert_eq!(
        get("launch.state_clear").2[..7],
        [0xc7, 0x46, 0x20, 0x00, 0x00, 0x00, 0x00]
    );
}

/// The committed `engine-facts.toml` rows match the image byte for byte, so
/// the table cannot silently drift from the image it cites.
#[test]
#[ignore = "requires CS_ENGINE_IMAGE"]
fn accept_t452_r01_the_committed_rows_match_the_engine_image() {
    let image = image();
    for (name, va, bytes) in launch_rows() {
        assert_eq!(
            at(&image, va, bytes.len()),
            bytes.as_slice(),
            "{name} at {va:#x}"
        );
    }
}

/// The image half: every pin's byte window still carries the measured
/// instruction sequence.
#[test]
#[ignore = "requires CS_ENGINE_IMAGE"]
fn accept_t452_r01_image_pins_the_launch_geometry() {
    let image = image();
    for (address, bytes, meaning) in IMAGE_PINS {
        assert_eq!(
            at(&image, address, bytes.len()),
            bytes,
            "0x{address:x} no longer carries: {meaning}"
        );
    }
    let text = text(&image);

    // The launcher: the spawn 0x5aef40 has exactly the six fire-site callers
    // found; the vector multiply-accumulate it uses is 0x5389a0.
    assert_eq!(
        direct_calls(text, 0x5a_ef40),
        [
            0x4a_b3cc, 0x4b_6e7b, 0x4b_7289, 0x4b_7559, 0x4c_01e0, 0x4c_0d59
        ],
        "the spawn's caller set changed"
    );
    for site in [0x5a_f15a, 0x5a_fe1b] {
        assert_eq!(
            at(&image, site, 5)[0],
            0xe8,
            "0x{site:x} must call the multiply-accumulate"
        );
        let rel = i32::from_le_bytes(at(&image, site + 1, 4).try_into().unwrap());
        assert_eq!(
            site.wrapping_add(5).wrapping_add_signed(i64::from(rel)),
            0x53_89a0
        );
    }
}
