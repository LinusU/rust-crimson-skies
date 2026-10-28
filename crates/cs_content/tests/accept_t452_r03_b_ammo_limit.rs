//! Task #2165 (T452-R.03-B): where `AMMO_LIMIT` goes in the engine and what
//! fills a slot's stack count.
//!
//! Task test prefix: `accept_t452_r03_b_`. Finding:
//! `docs/findings/2026-10-28-t452-r03-b-ammo-limit-and-slot-count.md`.

use std::collections::{BTreeMap, BTreeSet};

use cs_content::ordnance::{
    ORIGINAL_AMMO_LIMIT_DEFAULT, ORIGINAL_AMMO_LIMIT_DEFAULT_VA, ORIGINAL_AMMO_LIMIT_READ_VA,
    ORIGINAL_SLOT_COUNT_STORE_VA, OriginalWeaponRecord, classify_original_weapons,
    original_ammo_declarations, read_original_ammo_declarations,
};

const IMAGE_BASE: u64 = 0x40_0000;

fn record(name: &str, values: &[(&str, f64)]) -> OriginalWeaponRecord {
    OriginalWeaponRecord {
        record: name.to_owned(),
        keys: values
            .iter()
            .map(|(key, _)| (*key).to_owned())
            .collect::<BTreeSet<_>>(),
        first_values: values
            .iter()
            .map(|(key, value)| ((*key).to_owned(), *value))
            .collect::<BTreeMap<_, _>>(),
        second_values: BTreeMap::new(),
        first_texts: BTreeMap::new(),
    }
}

#[test]
fn accept_t452_r03_b_slot_count_is_cluster_size_not_ammo_limit() {
    let census = classify_original_weapons(vec![
        record("wep_04", &[("AMMO_LIMIT", 100.0), ("CLUSTER_SIZE", 3.0)]),
        record("wep_05", &[("CLUSTER_SIZE", 2.0)]),
        record("wep_27", &[("AMMO_LIMIT", 9999.0)]),
        record("wep_30", &[]),
    ]);
    let declared = original_ammo_declarations(&census);
    assert_eq!(declared.len(), 4);
    assert_eq!(declared[0].slot_count(), 3);
    assert_eq!(declared[0].parsed_ammo_limit(), 100.0);
    assert_eq!(declared[1].slot_count(), 2);
    assert_eq!(declared[1].parsed_ammo_limit(), 50.0);
    // AMMO_LIMIT alone gives no stack: the object is zero-filled.
    assert_eq!(declared[2].slot_count(), 0);
    assert_eq!(declared[2].parsed_ammo_limit(), 9999.0);
    assert_eq!(declared[3].slot_count(), 0);
    assert_eq!(
        declared[3].parsed_ammo_limit(),
        f64::from(ORIGINAL_AMMO_LIMIT_DEFAULT)
    );
}

fn image() -> Vec<u8> {
    let path = std::env::var_os("CS_ENGINE_IMAGE").expect(
        "CS_ENGINE_IMAGE is not set: this measurement needs the owner-supplied decrypted \
         executable",
    );
    std::fs::read(&path).expect("the engine image reads")
}

fn at(image: &[u8], va: u64, len: usize) -> &[u8] {
    let offset = usize::try_from(va - IMAGE_BASE).expect("offset");
    &image[offset..offset + len]
}

#[test]
#[ignore = "requires CS_ENGINE_IMAGE"]
fn accept_t452_r03_b_image_pins_the_parser_default_read_and_slot_store() {
    let image = image();
    // mov dword [edi], 0x42480000 with edi = ebx + 0x18.
    assert_eq!(
        at(&image, ORIGINAL_AMMO_LIMIT_DEFAULT_VA, 6),
        [0xc7, 0x07, 0x00, 0x00, 0x48, 0x42]
    );
    assert_eq!(ORIGINAL_AMMO_LIMIT_DEFAULT.to_bits(), 0x4248_0000);
    // push edi; push "AMMO_LIMIT"; push esi; call 0x57a130 (number read as float).
    assert_eq!(
        at(&image, ORIGINAL_AMMO_LIMIT_READ_VA - 7, 8),
        [0x57, 0x68, 0x04, 0xd2, 0x63, 0x00, 0x56, 0xe8]
    );
    assert_eq!(at(&image, 0x63_d204, 11), b"AMMO_LIMIT\0");
    assert_eq!(
        at(&image, 0x5a_d802, 3),
        [0x8d, 0x7b, 0x18],
        "edi = record + 0x18"
    );
    // CLUSTER_SIZE lookup, then mov [esi+0xc], edx into the zero-filled object.
    assert_eq!(
        at(&image, ORIGINAL_SLOT_COUNT_STORE_VA, 3),
        [0x89, 0x56, 0x0c]
    );
    assert_eq!(at(&image, 0x62_b344, 13), b"CLUSTER_SIZE\0");
    // push 0x38; mov edi,1; push edi; call calloc.
    assert_eq!(at(&image, 0x4b_a6f3, 5), [0x6a, 0x38, 0xbf, 0x01, 0x00]);
}

fn game_dir() -> std::path::PathBuf {
    std::path::PathBuf::from(std::env::var_os("CS_GAME_DIR").expect(
        "CS_GAME_DIR is not set: this measurement needs the original installation \
         (capability `retail`)",
    ))
}

#[test]
#[ignore = "requires CS_GAME_DIR"]
fn accept_t452_r03_b_retail_rocket_stacks_follow_cluster_size() {
    let declared = read_original_ammo_declarations(&game_dir()).expect("retail reads");
    let by_name = |name: &str| {
        declared
            .iter()
            .find(|row| row.record == name)
            .unwrap_or_else(|| panic!("{name} is not in the member"))
    };
    // Every selectable rocket type (wep_05..wep_15) states a stack through
    // CLUSTER_SIZE; none of them but wep_12 states AMMO_LIMIT, and wep_12's
    // AMMO_LIMIT (100) is not its stack (2).
    for index in 5..=15 {
        let row = by_name(&format!("wep_{index:02}"));
        assert!(row.slot_count() > 0, "{} has no stack", row.record);
        assert_eq!(row.ammo_limit.is_some(), index == 12, "{}", row.record);
    }
    let wep_12 = by_name("wep_12");
    assert_eq!((wep_12.ammo_limit, wep_12.slot_count()), (Some(100.0), 2));
    // The four listed spawn records and the two 9999 records: AMMO_LIMIT is not
    // the count the installer reads.
    for (name, limit, count) in [
        ("wep_04", 100.0, 1),
        ("wep_25", 100.0, 1),
        ("wep_26", 100.0, 1),
        ("wep_27", 9999.0, 1),
        ("wep_28", 100.0, 1),
        ("wep_23", 9999.0, 0),
        ("wep_29", 9999.0, 0),
    ] {
        let row = by_name(name);
        assert_eq!(
            (row.ammo_limit, row.slot_count()),
            (Some(limit), count),
            "{name}"
        );
    }
    // Where both keys are present on a gun record they are equal.
    for row in &declared {
        if let (Some(limit), Some(cluster)) = (row.ammo_limit, row.cluster_size)
            && cluster > 100.0
        {
            assert_eq!(limit, cluster, "{}", row.record);
        }
    }
}
