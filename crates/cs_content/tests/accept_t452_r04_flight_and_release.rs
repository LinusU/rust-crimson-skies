//! Task T452-R.04 (#2169, split of #2156): measure the original ordnance
//! flight duration and the weapon-to-mount release-interval binding.
//!
//! Task test prefix: `accept_t452_r04_`. Findings:
//! `docs/findings/2026-10-28-t452-r04-flight-duration-and-release-binding.md`.
//!
//! The pins are byte windows of the owner-supplied decrypted executable
//! (`$CS_ENGINE_IMAGE`, base `0x400000`): static-analysis evidence, never
//! original-run behaviour, and no code is printed or committed beyond the byte
//! strings in `data/owner-static/engine-facts.toml`. The synthetic half drives
//! the production `flight_duration` / `original_component_table_with_mounts`
//! readers and runs in CI; the retail half re-reads the installation and the
//! image and fails loudly without `CS_GAME_DIR` / `CS_ENGINE_IMAGE`.

use std::collections::{BTreeMap, BTreeSet};

use cs_content::ordnance::{
    OriginalFlightEnd, OriginalWeaponRecord, T452_04_NO_EXPIRES, T452_04_RELEASE_DELAY,
    classify_original_weapons, mounted_original_records, original_component_table,
    original_component_table_with_mounts, original_launch_geometry, original_mount_id,
    read_original_mount_rows, read_original_weapon_census,
};

const IMAGE_BASE: u64 = 0x40_0000;
const TEXT_START: u64 = 0x40_1000;
const TEXT_LEN: usize = 0x20_14bd;
const STATIONARY: [f64; 3] = [0.0, 0.0, 0.0];
const FORWARD: [f64; 3] = [0.0, 0.0, 1.0];

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

fn approx(actual: f64, expected: f64) {
    assert!(
        (actual - expected).abs() < 1e-3,
        "{actual} is not {expected}"
    );
}

// ---------------------------------------------------------- synthetic ----

#[test]
fn accept_t452_r04_full_inheritance_flight_is_range_over_closing_speed() {
    // A gun: no ACCELERATION, so v = shooter + dir*VELOCITY for the whole flight.
    let gun = original_launch_geometry(&record(
        "wep_90",
        &["VELOCITY", "RANGE"],
        &[("VELOCITY", 1000.0), ("RANGE", 1000.0)],
    ));
    let at_rest = gun.flight_duration(STATIONARY, FORWARD);
    assert_eq!(at_rest.end, OriginalFlightEnd::Range);
    approx(at_rest.seconds.expect("a range time"), 1.0);

    // A launcher moving 250 m/s along the launch direction adds to the speed.
    let fast = gun.flight_duration([0.0, 0.0, 250.0], FORWARD);
    approx(fast.seconds.expect("a range time"), 0.8);

    // The same launcher flying backwards relative to the shot slows it.
    let tail = gun.flight_duration([0.0, 0.0, -250.0], FORWARD);
    approx(tail.seconds.expect("a range time"), 1000.0 / 750.0);

    // A shot cancelled by the launcher's own velocity never travels.
    let hover = gun.flight_duration([0.0, 0.0, -1000.0], FORWARD);
    assert_eq!(hover.seconds, None);
    assert_eq!(hover.end, OriginalFlightEnd::Never);
}

#[test]
fn accept_t452_r04_absent_keys_fly_the_parser_defaults() {
    // wep_13's shape: no VELOCITY, RANGE or ACCELERATION -> 500 m at 120 m/s.
    let rocket = original_launch_geometry(&record("wep_91", &["ROCKET"], &[]));
    let flight = rocket.flight_duration(STATIONARY, FORWARD);
    assert_eq!(flight.end, OriginalFlightEnd::Range);
    approx(flight.seconds.expect("a range time"), 500.0 / 120.0);
}

#[test]
fn accept_t452_r04_ramping_flight_integrates_the_acceleration_curve() {
    // wep_04's shape from a stationary launcher: the speed ramps 150 m/s per
    // second to 450 m/s in 3 s (675 m), then 450 m/s for the remaining 225 m.
    let rocket = original_launch_geometry(&record(
        "wep_92",
        &["ROCKET", "ACCELERATION", "VELOCITY", "RANGE"],
        &[
            ("ACCELERATION", 150.0),
            ("VELOCITY", 450.0),
            ("RANGE", 900.0),
        ],
    ));
    let flight = rocket.flight_duration(STATIONARY, FORWARD);
    assert_eq!(flight.end, OriginalFlightEnd::Range);
    approx(flight.seconds.expect("a range time"), 3.5);

    // A short range is reached while still ramping: d = a*t^2/2.
    let short = original_launch_geometry(&record(
        "wep_93",
        &["ROCKET", "ACCELERATION", "VELOCITY", "RANGE"],
        &[
            ("ACCELERATION", 150.0),
            ("VELOCITY", 450.0),
            ("RANGE", 300.0),
        ],
    ));
    approx(
        short
            .flight_duration(STATIONARY, FORWARD)
            .seconds
            .expect("a range time"),
        2.0,
    );

    // The launcher's speed is the ramp's starting scalar and is carried to the
    // top speed: VELOCITY + |shooter|, whatever the direction.
    let moving = rocket.flight_duration([0.0, 100.0, 0.0], FORWARD);
    // start 100, +150/s: 100*t + 75 t^2 = 900 at t=2.4 (> 3 s? no: ramp ends at 3 s)
    // -> ramp end at 3 s covers 100*3 + 675 = 975 >= 900.
    let expected = (-100.0 + (100.0f64 * 100.0 + 2.0 * 150.0 * 900.0).sqrt()) / 150.0;
    approx(moving.seconds.expect("a range time"), expected);
}

#[test]
fn accept_t452_r04_a_detonation_timer_ends_the_flight_when_it_comes_first() {
    // wep_15's shape: VELOCITY 1, DETONATION_TIME 2, no RANGE -> the 500 m
    // default is 500 s away at rest, so the 2 s timer ends it.
    let slow = original_launch_geometry(&record(
        "wep_94",
        &["ROCKET", "VELOCITY", "ACCELERATION", "DETONATION_TIME"],
        &[
            ("VELOCITY", 1.0),
            ("ACCELERATION", 0.0),
            ("DETONATION_TIME", 2.0),
        ],
    ));
    let flight = slow.flight_duration(STATIONARY, FORWARD);
    assert_eq!(flight.end, OriginalFlightEnd::Timer);
    approx(flight.seconds.expect("the timer"), 2.0);

    // A fast launcher covers the 500 m default inside 2 s: range wins.
    let fast = slow.flight_duration([0.0, 0.0, 400.0], FORWARD);
    assert_eq!(fast.end, OriginalFlightEnd::Range);
    approx(fast.seconds.expect("a range time"), 500.0 / 401.0);

    // A parser default of -1.0 (or any non-positive value) is no timer.
    let none = original_launch_geometry(&record(
        "wep_95",
        &["DETONATION_TIME", "VELOCITY"],
        &[("DETONATION_TIME", -1.0), ("VELOCITY", 120.0)],
    ));
    assert_eq!(none.detonation_timer_s, None);
}

#[test]
fn accept_t452_r04_mines_and_instant_records_end_by_their_own_rules() {
    let mine = original_launch_geometry(&record("wep_96", &["MINE", "VELOCITY"], &[]));
    let flight = mine.flight_duration(STATIONARY, FORWARD);
    assert_eq!(flight.end, OriginalFlightEnd::Never);
    assert_eq!(flight.seconds, None);

    let timed_mine = original_launch_geometry(&record(
        "wep_97",
        &["MINE", "DETONATION_TIME"],
        &[("DETONATION_TIME", 8.0)],
    ));
    let flight = timed_mine.flight_duration(STATIONARY, FORWARD);
    assert_eq!(flight.end, OriginalFlightEnd::Timer);
    approx(flight.seconds.expect("the timer"), 8.0);

    let instant = original_launch_geometry(&record("wep_98", &["INSTANT", "RANGE"], &[]));
    let flight = instant.flight_duration(STATIONARY, FORWARD);
    assert_eq!(flight.end, OriginalFlightEnd::Instant);
    assert_eq!(flight.seconds, Some(0.0));
}

#[test]
fn accept_t452_r04_the_mount_id_is_the_two_digits_of_the_record_name() {
    assert_eq!(original_mount_id("wep_04"), 4);
    assert_eq!(original_mount_id("wep_15"), 15);
    assert_eq!(original_mount_id("wep_30"), 30);
    // The builder reads only name[4] and name[5]: a three-digit name keeps its
    // first two digits, so wep_130 collides with wep_13.
    assert_eq!(original_mount_id("wep_130"), 13);
    assert_eq!(original_mount_id("wep_140"), 14);
}

#[test]
fn accept_t452_r04_release_delay_stays_only_where_no_airframe_row_names_the_record() {
    let census = classify_original_weapons(vec![
        record(
            "wep_04",
            &["ROCKET", "LOCK_ON", "FIRE_RATE", "VELOCITY"],
            &[("FIRE_RATE", 1.0), ("VELOCITY", 450.0)],
        ),
        record(
            "wep_24",
            &["ROCKET", "LOCK_ON", "FIRE_RATE", "VELOCITY"],
            &[("FIRE_RATE", 3.0), ("VELOCITY", 450.0)],
        ),
    ]);
    let layout = {
        const HARDPOINTS: &str = "main\n{\ngui_create\n{\nobject DT[2]\nfor (Q = 0; Q < 2; Q++)\n{\nDT[Q] = @ctl@ZQ\n}\n}\n}\n";
        const ORDNANCE_LAYOUT: &str = "main\n{\ngui_create\n{\nobject QKA[4]\nfor (K = 0; K < 4; K++)\n{\nQKA[K] = @ctl@ZQ\n}\nobject RKA[8]\nfor (K = 0; K < 8; K++)\n{\nRKA[K] = @ctl@ZQ\n}\n}\ngui_init\n{\nfor (K = 0; K < 8; K++)\n{\nif (-1 == SLOT)\n{\ndeactivate(RKA[K])\n}\n}\nfor (K = 0; K < 4; K++)\n{\nif (-1 == SLOT)\n{\ndeactivate(QKA[K])\n}\n}\n}\n}\n";
        cs_content::ordnance::OriginalMountLayout::from_screens(HARDPOINTS, ORDNANCE_LAYOUT)
            .expect("the fixture layout reads")
    };

    // Without any airframe row the claim stands on both records.
    let bare = original_component_table(&census, &layout);
    for name in ["wep_04", "wep_24"] {
        assert!(
            bare.row(name)
                .expect("a row")
                .names_claim(T452_04_RELEASE_DELAY),
            "{name}"
        );
    }

    // A row that mounts wep_04 resolves its interval; wep_24 keeps the claim.
    let mounted = BTreeSet::from(["wep_04".to_owned()]);
    let table = original_component_table_with_mounts(&census, &layout, &mounted);
    assert!(
        !table
            .row("wep_04")
            .expect("a row")
            .names_claim(T452_04_RELEASE_DELAY)
    );
    assert!(
        table
            .row("wep_24")
            .expect("a row")
            .names_claim(T452_04_RELEASE_DELAY)
    );
    // Flight duration is derivable from the record's own keys, so the
    // lifetime claim is never recorded.
    assert!(!table.claims().contains(T452_04_NO_EXPIRES));
}

// ------------------------------------------------- the cited static rows ---

const TABLE: &str = include_str!("../../../data/owner-static/engine-facts.toml");

/// `(name, va, bytes)` of every `[[ordnance_flight_fact]]` row.
fn flight_rows() -> Vec<(String, u64, Vec<u8>)> {
    let field = |chunk: &str, name: &str| {
        chunk
            .lines()
            .find_map(|line| {
                let rest = line.strip_prefix(name)?.trim_start().strip_prefix('=')?;
                Some(rest.trim().trim_matches('"').to_string())
            })
            .unwrap_or_else(|| panic!("an ordnance_flight_fact row lacks {name}"))
    };
    TABLE
        .split("[[ordnance_flight_fact]]")
        .skip(1)
        .map(|chunk| chunk.split("\n[").next().unwrap_or(chunk))
        .map(|chunk| {
            let va = field(chunk, "va");
            let va = u64::from_str_radix(va.trim_start_matches("0x"), 16).expect("hex va");
            let hex = field(chunk, "bytes");
            let bytes = (0..hex.len() / 2)
                .map(|i| u8::from_str_radix(&hex[2 * i..2 * i + 2], 16).expect("hex byte"))
                .collect();
            assert_eq!(field(chunk, "provenance"), "static");
            (field(chunk, "name"), va, bytes)
        })
        .collect()
}

#[test]
fn accept_t452_r04_the_committed_rows_cite_every_measured_site() {
    let rows = flight_rows();
    let names: Vec<&str> = rows.iter().map(|(name, _, _)| name.as_str()).collect();
    assert_eq!(
        names,
        [
            "release.vehicle_weapons_key",
            "release.mount_id_from_name",
            "release.mount_count_from_row",
            "release.mount_interval_from_row",
            "release.mount_squares_from_row",
            "release.mount_stride",
            "release.launcher_next_release",
            "release.launcher_gate",
            "release.slot_timer_read",
            "release.slot_timer_halve",
            "release.record_interval_reader",
            "release.pool_allocate",
            "flight.spawn_ramp_scalars",
            "flight.mine_flag_parse",
            "flight.mine_skips_ramp",
            "flight.distance_accumulate",
            "flight.mine_halves_distance",
            "flight.detonation_timer",
            "flight.age_step",
        ]
    );
    let get = |name: &str| rows.iter().find(|(n, _, _)| n == name).expect("a row");
    // The heart of the binding: mount entry +0x14 is loaded from the row
    // element at +0x1c of the row's value array, and the count from +0x14.
    let interval = get("release.mount_interval_from_row");
    assert_eq!(interval.1, 0x4b_5a71);
    assert_eq!(interval.2[10..13], [0x8b, 0x4a, 0x1c]);
    assert_eq!(interval.2[13..], [0x89, 0x4c, 0x24, 0x28]);
    let count = get("release.mount_count_from_row");
    assert_eq!(count.2[6..9], [0x8b, 0x4a, 0x14]);
    // The record +0x14 reader divides RANGE/VELOCITY by it for the pool size.
    assert_eq!(
        get("release.record_interval_reader").2[6..15],
        [0xd9, 0x45, 0xf4, 0xd8, 0x73, 0x2c, 0xd8, 0x73, 0x14]
    );
}

/// The committed rows match the image byte for byte, so the table cannot
/// silently drift from the image it cites.
#[test]
#[ignore = "requires CS_ENGINE_IMAGE"]
fn accept_t452_r04_the_committed_rows_match_the_engine_image() {
    let image = image();
    for (name, va, bytes) in flight_rows() {
        assert_eq!(
            at(&image, va, bytes.len()),
            bytes.as_slice(),
            "{name} at {va:#x}"
        );
    }
}

/// The closed caller inventories the findings rest on.
#[test]
#[ignore = "requires CS_ENGINE_IMAGE"]
fn accept_t452_r04_image_pins_the_caller_inventories() {
    let image = image();
    let text = at(&image, TEXT_START, TEXT_LEN);
    // The mount builder has exactly one caller (the vehicle record loader).
    assert_eq!(direct_calls(text, 0x4b_59b0), [0x47_b089]);
    // The by-name weapon lookup's callers: the builder is one of them.
    assert!(direct_calls(text, 0x5a_bfd0).contains(&0x4b_5a25));
    // The only direct readers of weapon record +0x14 inside the weapon
    // module's parser are the FIRE_RATE store and the pool-size divide:
    // `fdiv dword [ebx+0x14]` appears exactly once there.
    let module = at(&image, 0x5a_c000, 0x3000);
    let divides = module
        .windows(3)
        .filter(|window| *window == [0xd8, 0x73, 0x14])
        .count();
    assert_eq!(divides, 1, "a second `fdiv [ebx+0x14]` appeared");
}

// --------------------------------------------------------------- retail ---

#[test]
#[ignore = "requires CS_GAME_DIR"]
fn accept_t452_r04_retail_rows_state_the_release_interval_per_airframe() {
    let install_root = std::env::var_os("CS_GAME_DIR")
        .map(std::path::PathBuf::from)
        .expect("CS_GAME_DIR is not set: this test needs the original installation");
    let rows = read_original_mount_rows(&install_root).expect("vehicle.zrd reads");
    let census =
        read_original_weapon_census(&install_root).expect("the installation's weapons.zrd reads");
    let geometry = cs_content::ordnance::original_launch_geometry_table(&census);

    // The player airframe's rows: the third element is the release interval.
    let player: BTreeMap<&str, f64> = rows
        .iter()
        .filter(|row| row.airframe == "player_airplane")
        .map(|row| (row.record.as_str(), row.release_interval_s))
        .collect();
    for (name, seconds) in [
        ("wep_00", 0.012),
        ("wep_03", 0.125),
        ("wep_04", 1.5),
        ("wep_13", 1.5),
        ("wep_14", 3.0),
        ("wep_25", 1.0),
    ] {
        approx(player[name], seconds);
    }

    // It is not weapon record +0x14: the same records state different
    // reciprocals of FIRE_RATE, so the rows and the records disagree.
    for name in ["wep_00", "wep_04", "wep_13", "wep_14"] {
        let record_interval = geometry[name].refire_interval_s.expect("FIRE_RATE");
        assert!(
            (record_interval - player[name]).abs() > 0.05,
            "{name}: the record's {record_interval} s equals the row's {} s",
            player[name]
        );
    }

    // AI airframes state their own intervals for the records they carry.
    let firebrand: Vec<(&str, f64)> = rows
        .iter()
        .filter(|row| row.airframe == "firebrand")
        .map(|row| (row.record.as_str(), row.release_interval_s))
        .collect();
    assert_eq!(firebrand.len(), 3);
    for ((name, seconds), (want_name, want_seconds)) in
        firebrand
            .iter()
            .zip([("wep_07", 200.0), ("wep_04", 200.0), ("wep_00", 0.06)])
    {
        assert_eq!(*name, want_name);
        approx(*seconds, want_seconds);
    }

    // Every row's mount id is the record number the builder derives.
    assert!(
        rows.iter()
            .all(|row| row.mount_id == original_mount_id(&row.record))
    );
    assert!(rows.len() > 100, "the vehicle document carries many rows");
}

#[test]
#[ignore = "requires CS_GAME_DIR"]
fn accept_t452_r04_retail_component_rows_keep_the_claim_only_without_an_airframe_row() {
    let install_root = std::env::var_os("CS_GAME_DIR")
        .map(std::path::PathBuf::from)
        .expect("CS_GAME_DIR is not set: this test needs the original installation");
    let rows = read_original_mount_rows(&install_root).expect("vehicle.zrd reads");
    let mounted = mounted_original_records(&rows);
    let table = cs_content::ordnance::read_original_component_table(&install_root)
        .expect("the table reads");

    // The lifetime claim is gone from every row: the rule is derivable.
    assert!(!table.claims().contains(T452_04_NO_EXPIRES));

    let claimed: BTreeSet<&str> = table
        .rows
        .iter()
        .filter(|row| row.names_claim(T452_04_RELEASE_DELAY))
        .map(|row| row.record.as_str())
        .collect();
    let unmounted: BTreeSet<&str> = table
        .rows
        .iter()
        .filter(|row| row.fire_rate.is_some() && !mounted.contains(&row.record))
        .map(|row| row.record.as_str())
        .collect();
    assert_eq!(claimed, unmounted);
    assert_eq!(claimed, BTreeSet::from(["wep_24", "wep_26"]));
}

#[test]
#[ignore = "requires CS_GAME_DIR"]
fn accept_t452_r04_retail_flight_durations_follow_the_rule() {
    let install_root = std::env::var_os("CS_GAME_DIR")
        .map(std::path::PathBuf::from)
        .expect("CS_GAME_DIR is not set: this test needs the original installation");
    let census =
        read_original_weapon_census(&install_root).expect("the installation's weapons.zrd reads");
    let geometry = cs_content::ordnance::original_launch_geometry_table(&census);

    // From a stationary launcher.
    let at_rest = |name: &str| geometry[name].flight_duration(STATIONARY, FORWARD);
    // wep_04: ACCELERATION 150, VELOCITY 450, RANGE 900.
    approx(at_rest("wep_04").seconds.expect("a time"), 3.5);
    // wep_13: no VELOCITY/RANGE/ACCELERATION -> 500 m at 120 m/s.
    approx(at_rest("wep_13").seconds.expect("a time"), 500.0 / 120.0);
    // wep_15: VELOCITY 1, DETONATION_TIME 2 -> the timer.
    assert_eq!(at_rest("wep_15").end, OriginalFlightEnd::Timer);
    approx(at_rest("wep_15").seconds.expect("a time"), 2.0);
    // wep_00: 1000 m at 1000 m/s.
    approx(at_rest("wep_00").seconds.expect("a time"), 1.0);

    // No retail record is a mine or INSTANT, so every record ends by range or
    // by its timer, and every duration is finite.
    for (name, row) in &geometry {
        assert!(!row.mine, "{name}");
        let flight = row.flight_duration(STATIONARY, FORWARD);
        assert!(
            matches!(
                flight.end,
                OriginalFlightEnd::Range | OriginalFlightEnd::Timer
            ),
            "{name}: {flight:?}"
        );
        assert!(flight.seconds.is_some_and(f64::is_finite), "{name}");
    }
}
