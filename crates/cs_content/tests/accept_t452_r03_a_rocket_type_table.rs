//! Task #2164 (T452-R.03-A): the rocket id -> `weapons.zrd` record table the
//! engine's id mapper (`0x4440f0`, jump table `0x444148`) encodes, resolved
//! through the production reader.
//!
//! Task test prefix: `accept_t452_r03_a_`. Finding:
//! `docs/findings/2026-10-28-t452-r03-rocket-type-map-and-purchase-totals.md`.
//!
//! The synthetic half hand-builds records; the retail half re-reads the
//! installation (`CS_GAME_DIR`) and the image (`CS_ENGINE_IMAGE`, base
//! `0x400000`) and fails loudly without them.

use std::collections::{BTreeMap, BTreeSet};

use cs_content::ordnance::{
    ORIGINAL_ROCKET_ID_FUNCTION_VA, ORIGINAL_ROCKET_ID_JUMP_TABLE_VA,
    ORIGINAL_ROCKET_ID_WEAPON_INDEX, OriginalWeaponRecord, classify_original_weapons,
    original_rocket_type_table, original_rocket_weapon_record, read_original_rocket_type_table,
};

const IMAGE_BASE: u64 = 0x40_0000;

fn named(record: &str, name: &str) -> OriginalWeaponRecord {
    OriginalWeaponRecord {
        record: record.to_owned(),
        keys: BTreeSet::from(["NAME".to_owned()]),
        first_values: BTreeMap::new(),
        second_values: BTreeMap::new(),
        first_texts: BTreeMap::from([("NAME".to_owned(), name.to_owned())]),
    }
}

#[test]
fn accept_t452_r03_a_ids_resolve_to_the_measured_records() {
    let records: Vec<_> = (5..=15)
        .map(|index| named(&format!("wep_{index:02}"), &format!("N{index}")))
        .collect();
    let table = original_rocket_type_table(&classify_original_weapons(records));
    assert!(table.missing_records.is_empty());
    let by_id: Vec<_> = table
        .rows
        .iter()
        .map(|row| (row.rocket_id, row.record.as_str()))
        .collect();
    assert_eq!(by_id[0], (0, "wep_05"));
    assert_eq!(by_id[5], (5, "wep_15"));
    assert_eq!(by_id[6], (6, "wep_13"));
    assert_eq!(by_id[7], (7, "wep_12"));
    assert_eq!(by_id[10], (10, "wep_14"));
    assert_eq!(table.rows[6].short_name.as_deref(), Some("N13"));
}

#[test]
fn accept_t452_r03_a_none_id_and_beyond_select_no_record() {
    assert_eq!(original_rocket_weapon_record(10).as_deref(), Some("wep_14"));
    assert_eq!(original_rocket_weapon_record(11), None);
    assert_eq!(original_rocket_weapon_record(u32::MAX), None);
}

#[test]
fn accept_t452_r03_a_a_record_the_member_lacks_is_named() {
    let records: Vec<_> = (5..=15)
        .filter(|index| *index != 9)
        .map(|index| named(&format!("wep_{index:02}"), "N"))
        .collect();
    let table = original_rocket_type_table(&classify_original_weapons(records));
    assert_eq!(table.missing_records, ["wep_09"]);
    assert_eq!(table.rows.len(), 10);
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

/// Re-measures the table: the mapper range-checks the id against 10 and jumps
/// through the dword table; every target is `mov eax, N; ret`.
#[test]
#[ignore = "requires CS_ENGINE_IMAGE"]
fn accept_t452_r03_a_image_pins_the_id_map() {
    let image = image();
    // mov ecx,[esp+4]; or eax,-1; cmp ecx,10; ja ..; jmp [ecx*4 + 0x444148]
    assert_eq!(
        at(&image, ORIGINAL_ROCKET_ID_FUNCTION_VA, 19),
        [
            0x8b, 0x4c, 0x24, 0x04, 0x83, 0xc8, 0xff, 0x83, 0xf9, 0x0a, 0x77, 0x48, 0xff, 0x24,
            0x8d, 0x48, 0x41, 0x44, 0x00
        ]
    );
    let table = at(&image, ORIGINAL_ROCKET_ID_JUMP_TABLE_VA, 44);
    for (id, chunk) in table.as_chunks::<4>().0.iter().enumerate() {
        let target = u64::from(u32::from_le_bytes(*chunk));
        let body = at(&image, target, 6);
        assert_eq!(body[0], 0xb8, "id {id}: mov eax, imm32");
        let index = u32::from_le_bytes(body[1..5].try_into().expect("4 bytes"));
        assert_eq!(body[5], 0xc3, "id {id}: ret");
        assert_eq!(index, ORIGINAL_ROCKET_ID_WEAPON_INDEX[id], "id {id}");
    }
}

/// The slot count comes from the record named `wep_%02d`: `0x4bad90` formats
/// the name and returns `(record + 0x210) + 0xc`.
#[test]
#[ignore = "requires CS_ENGINE_IMAGE"]
fn accept_t452_r03_a_image_pins_the_slot_count_source() {
    let image = image();
    let bytes = at(&image, 0x4b_ad90, 0x4a);
    // push offset "wep_%02d"; ... mov edx,[eax+0x210]; mov eax,[edx+0xc]
    assert_eq!(&bytes[23..28], [0x68, 0x94, 0xb3, 0x62, 0x00]);
    assert_eq!(&bytes[56..62], [0x8b, 0x90, 0x10, 0x02, 0x00, 0x00]);
    assert_eq!(&bytes[62..65], [0x8b, 0x42, 0x0c]);
    assert_eq!(at(&image, 0x62_b394, 8), b"wep_%02d");
}

fn game_dir() -> std::path::PathBuf {
    let dir = std::env::var_os("CS_GAME_DIR").expect(
        "CS_GAME_DIR is not set: this measurement needs the original installation \
         (capability `retail`)",
    );
    std::path::PathBuf::from(dir)
}

#[test]
#[ignore = "requires CS_GAME_DIR"]
fn accept_t452_r03_a_retail_every_selectable_type_resolves_to_a_named_record() {
    let table = read_original_rocket_type_table(&game_dir()).expect("retail reads");
    assert!(
        table.missing_records.is_empty(),
        "{:?}",
        table.missing_records
    );
    assert_eq!(table.rows.len(), 11);
    for row in &table.rows {
        let name = row.short_name.as_deref().unwrap_or("");
        assert!(!name.is_empty(), "{} declares no NAME", row.record);
    }
    assert_eq!(
        table.spawn_only,
        ["wep_04", "wep_24", "wep_25", "wep_26", "wep_27"]
    );
}
