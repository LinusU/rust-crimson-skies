//! Task #2170 (T452.R05): the measured record-to-slot loadout map and the
//! `SONIC`/`FLASH` incapacitation status.
//!
//! Task test prefix: `accept_t452_r05_`. Finding:
//! `docs/findings/2026-10-28-t452-r05-slot-map-and-incapacitation.md`.
//!
//! The pure tests call the production measurement on hand-built rows; the
//! retail test runs the same production functions on the owner's installation;
//! the image tests re-read every byte window the measured facts cite from
//! `$CS_ENGINE_IMAGE` (static analysis, never original-run behavior).

use cs_content::ordnance::{
    DeclaredOrdnanceFamily, MeasuredStatusKind, ORIGINAL_ROCKET_ID_FUNCTION_VA,
    ORIGINAL_ROCKET_SLOT_COUNT, OriginalAreaDeclaration, OriginalClusterShape,
    OriginalComponentLaunch, OriginalComponentTable, OriginalDamageDeclaration, OriginalLifetime,
    OriginalMountDeclaration, OriginalSlotReach, OriginalStatusEffectDeclaration, T452_04_MOUNT,
    T452_04_STATUS_DURATION, T452_R05_SCRIPT_AMMO_USE, T452_R05_STATUS_INCAPACITATION,
    T452_R05_STATUS_KIND_FOLLOW_UP, measure_loadout_slot, measure_loadout_slots,
    measure_status_effects, original_rocket_ids_for_weapon_index, original_rocket_weapon_record,
    original_weapon_record_index, read_original_component_table,
};

/// The image base the executable's virtual addresses are relative to.
const IMAGE_BASE: u64 = 0x40_0000;

fn rocket_slot() -> OriginalMountDeclaration {
    OriginalMountDeclaration::RocketSlot { slots: 8 }
}

fn unknown_slot() -> OriginalMountDeclaration {
    OriginalMountDeclaration::Unknown {
        claim: T452_04_MOUNT.to_owned(),
        reason: "no screen states a slot".to_owned(),
    }
}

fn status(key: &'static str, duration_s: Option<f64>) -> OriginalStatusEffectDeclaration {
    OriginalStatusEffectDeclaration {
        key,
        duration_s,
        duration_unit: duration_s.map(|_| "TIME".to_owned()),
    }
}

fn row(
    record: &str,
    family: DeclaredOrdnanceFamily,
    mount: OriginalMountDeclaration,
    statuses: Vec<OriginalStatusEffectDeclaration>,
) -> OriginalComponentLaunch {
    OriginalComponentLaunch {
        record: record.to_owned(),
        family,
        mount,
        velocity: None,
        fire_rate: None,
        proximity_trigger_m: None,
        lifetime: OriginalLifetime::Undeclared,
        area: OriginalAreaDeclaration::default(),
        damage: OriginalDamageDeclaration::default(),
        statuses,
        cluster: OriginalClusterShape::Volley { munitions: 1 },
        unknowns: Vec::new(),
    }
}

// -------------------------------------------------------- the slot map ---

#[test]
fn accept_t452_r05_the_record_index_is_the_number_in_its_name() {
    assert_eq!(original_weapon_record_index("wep_25"), Some(25));
    assert_eq!(original_weapon_record_index("wep_05"), Some(5));
    assert_eq!(original_weapon_record_index("wep_130"), Some(130));
    assert_eq!(original_weapon_record_index("wep_"), None);
    assert_eq!(original_weapon_record_index("gun_05"), None);
}

#[test]
fn accept_t452_r05_a_slot_holds_a_rocket_type_not_a_record() {
    // The plane record's eight rocket slots carry rocket type ids 0..=11, and
    // 0x4440f0 maps exactly ids 0..=10 to record indices 5..=15.
    assert_eq!(original_rocket_ids_for_weapon_index(5), [0]);
    assert_eq!(original_rocket_ids_for_weapon_index(15), [5]);
    assert_eq!(original_rocket_ids_for_weapon_index(12), [7]);
    assert_eq!(original_rocket_ids_for_weapon_index(14), [10]);
    // No record outside that range is selected by any id, `wep_25` included.
    for index in [0, 1, 2, 3, 4, 16, 23, 24, 25, 26, 27, 28, 130] {
        assert_eq!(
            original_rocket_ids_for_weapon_index(index),
            [],
            "wep_{index:02} must have no rocket type id"
        );
    }
}

#[test]
fn accept_t452_r05_wep_25_has_no_plane_record_slot() {
    let glidebomb = row(
        "wep_25",
        DeclaredOrdnanceFamily::GuidedRocket,
        unknown_slot(),
        Vec::new(),
    );
    let measured = measure_loadout_slot(&glidebomb);
    assert_eq!(measured.record, "wep_25");
    assert_eq!(measured.weapon_index, Some(25));
    assert_eq!(
        measured.reach,
        OriginalSlotReach::NoRocketSlot {
            script_index_writer: true
        }
    );
    // The T452.04 mount claim is resolved: the value behind it is "no slot".
    assert!(measured.resolves_claim(T452_04_MOUNT));
    assert!(!measured.names_claim(T452_04_MOUNT));
    // What a mission script does with the raw index is script content.
    assert!(measured.names_claim(T452_R05_SCRIPT_AMMO_USE));
}

#[test]
fn accept_t452_r05_a_selectable_record_names_the_ids_that_carry_it() {
    let smoke = row(
        "wep_13",
        DeclaredOrdnanceFamily::AreaDenialEngine,
        rocket_slot(),
        vec![status("SMOKE_SCREEN", Some(8.0))],
    );
    assert_eq!(
        measure_loadout_slot(&smoke).reach,
        OriginalSlotReach::RocketSlot {
            slots: ORIGINAL_ROCKET_SLOT_COUNT,
            rocket_ids: vec![6],
        }
    );
    let torpedo = row(
        "wep_14",
        DeclaredOrdnanceFamily::AerialTorpedo,
        rocket_slot(),
        Vec::new(),
    );
    assert_eq!(
        measure_loadout_slot(&torpedo).reach,
        OriginalSlotReach::RocketSlot {
            slots: ORIGINAL_ROCKET_SLOT_COUNT,
            rocket_ids: vec![10],
        }
    );
}

#[test]
fn accept_t452_r05_exactly_the_five_spawn_only_records_have_no_slot() {
    // The sixteen records T452.01 classifies, by family.
    let classified = [
        ("wep_04", DeclaredOrdnanceFamily::GuidedRocket),
        ("wep_05", DeclaredOrdnanceFamily::GuidedRocket),
        ("wep_06", DeclaredOrdnanceFamily::GuidedRocket),
        ("wep_07", DeclaredOrdnanceFamily::GuidedRocket),
        ("wep_08", DeclaredOrdnanceFamily::GuidedRocket),
        ("wep_09", DeclaredOrdnanceFamily::GuidedRocket),
        ("wep_10", DeclaredOrdnanceFamily::GuidedRocket),
        ("wep_11", DeclaredOrdnanceFamily::GuidedRocket),
        ("wep_12", DeclaredOrdnanceFamily::AreaDenialEngine),
        ("wep_13", DeclaredOrdnanceFamily::AreaDenialEngine),
        ("wep_14", DeclaredOrdnanceFamily::AerialTorpedo),
        ("wep_15", DeclaredOrdnanceFamily::GuidedRocket),
        ("wep_24", DeclaredOrdnanceFamily::GuidedRocket),
        ("wep_25", DeclaredOrdnanceFamily::GuidedRocket),
        ("wep_26", DeclaredOrdnanceFamily::ProximityFlak),
        ("wep_27", DeclaredOrdnanceFamily::GuidedRocket),
    ];
    let mut selectable = Vec::new();
    let mut unselectable = Vec::new();
    for (record, family) in classified {
        let mount = if record == "wep_25" {
            unknown_slot()
        } else {
            rocket_slot()
        };
        let measured = measure_loadout_slot(&row(record, family, mount, Vec::new()));
        match measured.reach {
            OriginalSlotReach::RocketSlot { slots, .. } => {
                assert_eq!(slots, ORIGINAL_ROCKET_SLOT_COUNT, "{record}");
                selectable.push(record);
            }
            OriginalSlotReach::NoRocketSlot { .. } => unselectable.push(record),
        }
    }
    assert_eq!(
        selectable,
        [
            "wep_05", "wep_06", "wep_07", "wep_08", "wep_09", "wep_10", "wep_11", "wep_12",
            "wep_13", "wep_14", "wep_15",
        ]
    );
    assert_eq!(
        unselectable,
        ["wep_04", "wep_24", "wep_25", "wep_26", "wep_27"]
    );
}

// -------------------------------------------------- the effect status ---

#[test]
fn accept_t452_r05_sonic_and_flash_are_a_pilot_incapacitation() {
    let sonic = row(
        "wep_08",
        DeclaredOrdnanceFamily::GuidedRocket,
        rocket_slot(),
        vec![status("SONIC", None)],
    );
    let measured = measure_status_effects(&sonic);
    assert_eq!(measured.len(), 1);
    let effect = &measured[0];
    assert_eq!(effect.record, "wep_08");
    assert_eq!(effect.key, "SONIC");
    assert_eq!(effect.kind, MeasuredStatusKind::PilotIncapacitation);
    assert_eq!(effect.duration.max_seconds, 5.0);
    assert_eq!(effect.duration.full_intensity_ratio, 0.6);
    assert_eq!(effect.duration.zero_intensity_ratio, 1.0);
    assert!(
        !effect.duration.facing_scaled,
        "SONIC skips the facing scale"
    );
    assert!(
        effect
            .duration
            .ratio_denominator
            .contains("IMPACT_PROXIMITY"),
        "the ratio is measured over the record's blast radius"
    );
    // The player gets the screen and sound effect, never the controls.
    assert!(effect.victims.player.screen_effect);
    assert!(effect.victims.player.sound);
    assert!(!effect.victims.player.control_loss);
    // Every other struck actor loses control for the duration.
    assert_eq!(effect.victims.other_actor.mode, 4);
    assert_eq!(effect.victims.other_actor.control_words_zeroed, 6);
    assert!(effect.victims.gate.contains("+0x91d"));
    assert!(effect.victims.gate.contains("+0xf8"));
    assert!(effect.victims.gate.contains("+0x67c"));
    assert!(effect.shot_consumed);
    // The duration is measured; the runtime kind is not expressible.
    assert!(effect.resolves_claim(T452_04_STATUS_DURATION));
    assert!(effect.names_claim(T452_R05_STATUS_INCAPACITATION));
    assert_eq!(effect.runtime_kind, None);
    assert_eq!(effect.follow_up_task, Some(T452_R05_STATUS_KIND_FOLLOW_UP));
}

#[test]
fn accept_t452_r05_flash_adds_the_facing_scale_and_sonic_does_not() {
    let flash = row(
        "wep_09",
        DeclaredOrdnanceFamily::GuidedRocket,
        rocket_slot(),
        vec![status("FLASH", None)],
    );
    let measured = measure_status_effects(&flash);
    assert_eq!(measured.len(), 1);
    assert_eq!(measured[0].key, "FLASH");
    assert!(measured[0].duration.facing_scaled);

    let sonic = row(
        "wep_08",
        DeclaredOrdnanceFamily::GuidedRocket,
        rocket_slot(),
        vec![status("SONIC", None)],
    );
    let measured = measure_status_effects(&sonic);
    assert_eq!(measured.len(), 1);
    assert!(
        !measured[0].duration.facing_scaled,
        "SONIC is not scaled by the victim's facing"
    );

    // A record that declares neither bit measures nothing: the layer never
    // invents an effect for a record whose keys do not state one.
    let plain = row(
        "wep_07",
        DeclaredOrdnanceFamily::GuidedRocket,
        rocket_slot(),
        Vec::new(),
    );
    assert!(measure_status_effects(&plain).is_empty());
}

#[test]
fn accept_t452_r05_both_bits_measured_are_the_same_incapacitation() {
    // `wep_08` declares SONIC and `wep_09`/`wep_15` declare FLASH; the pair on
    // one row must measure two rows of the same kind.
    let both = row(
        "wep_09",
        DeclaredOrdnanceFamily::GuidedRocket,
        rocket_slot(),
        vec![status("SONIC", None), status("FLASH", None)],
    );
    let measured = measure_status_effects(&both);
    assert_eq!(measured.len(), 2);
    for effect in &measured {
        assert_eq!(effect.kind, MeasuredStatusKind::PilotIncapacitation);
        assert_eq!(effect.runtime_kind, None);
        assert_eq!(effect.duration.facing_scaled, effect.key == "FLASH");
    }
}

// ------------------------------------------------------ the image pins ---

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

/// `(virtual address, bytes, what the window carries)` of the loadout map.
const SLOT_PINS: [(u64, &[u8], &str); 10] = [
    (
        0x0044_3f08,
        &[0xbe, 0x38, 0xae, 0x62, 0x00],
        "the binder targets the eight rocket slots 0x62ae38",
    ),
    (
        0x0044_3f0d,
        &[0x8d, 0xba, 0xa8, 0x00, 0x00, 0x00],
        "the binder reads the plane record's rocket ids at +0xa8",
    ),
    (
        0x0044_3f13,
        &[0x8b, 0x07, 0x50, 0xe8, 0xd5, 0x01, 0x00, 0x00, 0x89, 0x06],
        "each plane-record id goes through 0x4440f0 and its answer is stored in the slot array",
    ),
    (
        0x0044_3f26,
        &[0x81, 0xfe, 0x58, 0xae, 0x62, 0x00],
        "the loop ends at 0x62ae58: exactly eight rocket slots",
    ),
    (
        0x0044_40f4,
        &[0x83, 0xc8, 0xff, 0x83, 0xf9, 0x0a, 0x77, 0x48],
        "the id mapper answers -1 for any id above 10",
    ),
    (
        0x0044_4269,
        &[
            0x8b, 0x96, 0x78, 0x30, 0x64, 0x00, 0x52, 0xe8, 0x7b, 0xfe, 0xff, 0xff,
        ],
        "the preset binder maps its eight rocket ids through the same 0x4440f0",
    ),
    (
        0x004b_250b,
        &[0x83, 0xfe, 0x0c, 0x7d, 0x24],
        "the installer's slot index runs 0..11: four gun slots then eight rocket slots",
    ),
    (
        0x0043_e553,
        &[0x89, 0x04, 0xb5, 0x28, 0xae, 0x62, 0x00],
        "the script command's two-argument form stores a raw record index into the slot table",
    ),
    (
        0x0043_e56a,
        &[
            0xb9, 0x08, 0x00, 0x00, 0x00, 0xbf, 0x38, 0xae, 0x62, 0x00, 0xf3, 0xab,
        ],
        "the one-argument form fills all eight rocket slots with the raw record index",
    ),
    (
        0x005b_7980,
        &[0xe8, 0x6b, 0xff, 0xff, 0xff],
        "the script argument reader: no range check on the index it returns",
    ),
];

/// `(virtual address, bytes, what the window carries)` of the exposure.
const EXPOSURE_PINS: [(u64, &[u8], &str); 18] = [
    (
        0x004b_a726,
        &[
            0x68, 0x68, 0xb2, 0x62, 0x00, 0x53, 0xe8, 0x5f, 0xf9, 0x0b, 0x00, 0x83, 0xc4, 0x08,
            0x85, 0xc0, 0x74, 0x06,
        ],
        "the weapon parser reads the SONIC key and its store follows",
    ),
    (
        0x004b_a734,
        &[0x85, 0xc0, 0x74, 0x06, 0x81, 0x0e, 0x00, 0x02, 0x00, 0x00],
        "SONIC sets flag 0x200 on the weapon's effect object",
    ),
    (
        0x004b_a74c,
        &[0x85, 0xc0, 0x74, 0x06, 0x81, 0x0e, 0x00, 0x08, 0x00, 0x00],
        "FLASH sets flag 0x800",
    ),
    (
        0x004b_9d84,
        &[
            0x8b, 0x88, 0x10, 0x02, 0x00, 0x00, 0x8b, 0x09, 0xf6, 0xc5, 0x0a, 0x0f, 0x85, 0xcf,
            0x07, 0x00, 0x00,
        ],
        "the per-hit routine loads the flag word and tests ch&0x0a (SONIC|FLASH)",
    ),
    (
        0x0042_e84a,
        &[0x8a, 0x86, 0x1d, 0x09, 0x00, 0x00, 0x84, 0xc0],
        "the exposure returns at once for a victim whose +0x91d is set",
    ),
    (
        0x0042_e85b,
        &[0xd9, 0x45, 0x18, 0xc6, 0x45, 0x13, 0x00, 0xd8, 0x75, 0x1c],
        "the exposure divides its numerator by its denominator (ratio)",
    ),
    (
        0x0042_e87e,
        &[
            0xd8, 0x25, 0x1c, 0x41, 0x60, 0x00, 0xd8, 0x0d, 0xa8, 0x40, 0x60, 0x00, 0xd8, 0x2d,
            0xdc, 0x32, 0x60, 0x00,
        ],
        "the falloff: -(0.6), *2.5, 1 - value",
    ),
    (
        0x0042_e9b1,
        &[0xd8, 0x0d, 0xbc, 0x36, 0x60, 0x00],
        "the duration is intensity * 5.0 s",
    ),
    (
        0x005a_dd6a,
        &[
            0x89, 0x4b, 0x3c, 0x8b, 0x50, 0x04, 0xd9, 0x42, 0x0c, 0xd9, 0xc0, 0xd8, 0xc9, 0xd9,
            0x5b, 0x40,
        ],
        "IMPACT_PROXIMITY is stored at record +0x3c and its square at +0x40",
    ),
    (
        0x004b_a590,
        &[
            0xd9, 0x47, 0x3c, 0x8d, 0x55, 0x20, 0x52, 0xd9, 0xc0, 0x8b, 0x55, 0xec, 0xd8, 0xc9,
            0x8d, 0x45, 0x14, 0x50, 0x8b, 0x45, 0x10, 0x51,
        ],
        "the call site loads IMPACT_PROXIMITY and squares it for the exposure",
    ),
    (
        0x004b_a5a6,
        &[
            0x8b, 0x4d, 0x0c, 0xd9, 0x1c, 0x24, 0x51, 0x53, 0xdd, 0xd8, 0x56, 0x52, 0x50,
        ],
        "the measured distance and the squared radius are the exposure's ratio arguments",
    ),
    (
        0x004b_a5bf,
        &[0x3b, 0x35, 0x98, 0xc2, 0x71, 0x00, 0x75, 0x60],
        "the player actor is branched on before the actor path",
    ),
    (
        0x004b_a5cf,
        &[0xf6, 0xc4, 0x02, 0xc7, 0x45, 0xdc, 0x00, 0x00, 0x80, 0x3f],
        "SONIC selects the player-side parameter triple",
    ),
    (
        0x0042_00ea,
        &[
            0x3b, 0x35, 0x98, 0xc2, 0x71, 0x00, 0x0f, 0x84, 0x95, 0x00, 0x00, 0x00, 0x38, 0x9e,
            0xf8, 0x00, 0x00, 0x00, 0x0f, 0x85, 0x89, 0x00, 0x00, 0x00,
        ],
        "the incapacitation gate: not the player and victim +0xf8 == 0",
    ),
    (
        0x0042_0102,
        &[
            0x8b, 0x86, 0x7c, 0x06, 0x00, 0x00, 0x3b, 0xc3, 0x74, 0x05, 0x83, 0xf8, 0x04,
        ],
        "the gate accepts only actor words 0 and 4",
    ),
    (
        0x0042_0127,
        &[0xc7, 0x86, 0x58, 0x03, 0x00, 0x00, 0x04, 0x00, 0x00, 0x00],
        "the struck actor's mode word becomes 4",
    ),
    (
        0x0048_9acd,
        &[
            0x83, 0xbe, 0x58, 0x03, 0x00, 0x00, 0x04, 0x75, 0x1d, 0xd9, 0x05, 0x70, 0xc4, 0x71,
            0x00,
        ],
        "the recovery site compares the shared clock with the actor's stored timestamp",
    ),
    (
        0x004b_a632,
        &[0x8b, 0x45, 0x1c, 0xc7, 0x40, 0x04, 0x00, 0x00, 0x00, 0x00],
        "the exposure consumes the shot: both result floats are zeroed",
    ),
];

#[test]
#[ignore = "requires CS_ENGINE_IMAGE"]
fn accept_t452_r05_the_image_pins_the_loadout_map() {
    let image = image();
    for (va, bytes, meaning) in SLOT_PINS {
        assert_eq!(
            at(&image, va, bytes.len()),
            bytes,
            "0x{va:x} no longer carries: {meaning}"
        );
    }
    assert_eq!(
        at(&image, ORIGINAL_ROCKET_ID_FUNCTION_VA, 4),
        [0x8b, 0x4c, 0x24, 0x04],
        "the id mapper's head"
    );
}

#[test]
#[ignore = "requires CS_ENGINE_IMAGE"]
fn accept_t452_r05_the_image_pins_the_exposure() {
    let image = image();
    for (va, bytes, meaning) in EXPOSURE_PINS {
        assert_eq!(
            at(&image, va, bytes.len()),
            bytes,
            "0x{va:x} no longer carries: {meaning}"
        );
    }
    // The constants the measured formula cites, read from the image: 0.6, 2.5,
    // 5.0, 1.0 and the zero the falloff clamps at.
    let constant = |va: u64| f32::from_le_bytes(at(&image, va, 4).try_into().expect("four bytes"));
    assert_eq!(constant(0x0060_411c), 0.6, "full-intensity ratio");
    assert_eq!(constant(0x0060_40a8), 2.5, "falloff slope");
    assert_eq!(constant(0x0060_36bc), 5.0, "duration ceiling");
    assert_eq!(constant(0x0060_32dc), 1.0, "zero-intensity ratio");
    assert_eq!(constant(0x0060_32c8), 0.0, "the clamp");
    // The recovery site's compare and clear.
    assert_eq!(
        at(&image, 0x0048_9adc, 11),
        [
            0xd8, 0x9e, 0xc0, 0x00, 0x00, 0x00, 0xdf, 0xe0, 0xf6, 0xc4, 0x01
        ],
        "fcomp [esi+0xc0] then test ah,1"
    );
    assert_eq!(
        at(&image, 0x0048_9ae9, 10),
        [0xc7, 0x86, 0x58, 0x03, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00],
        "the recovery site clears mode 4"
    );
}

// ------------------------------------------------------------- retail ---

fn game_dir() -> std::path::PathBuf {
    let dir = std::env::var_os("CS_GAME_DIR").expect(
        "CS_GAME_DIR is not set: this measurement needs the original installation \
         (capability `retail`)",
    );
    std::path::PathBuf::from(dir)
}

#[test]
#[ignore = "requires CS_GAME_DIR"]
fn accept_t452_r05_retail_the_slot_map_and_the_status_match_the_records() {
    let table: OriginalComponentTable =
        read_original_component_table(&game_dir()).expect("the component table reads");
    let slots = measure_loadout_slots(&table);
    assert_eq!(slots.len(), 16, "sixteen classified records");

    let wep25 = slots
        .iter()
        .find(|slot| slot.record == "wep_25")
        .expect("wep_25 is classified");
    assert_eq!(
        wep25.reach,
        OriginalSlotReach::NoRocketSlot {
            script_index_writer: true
        }
    );
    assert!(wep25.resolves_claim(T452_04_MOUNT));

    let mut unselectable: Vec<&str> = slots
        .iter()
        .filter(|slot| matches!(slot.reach, OriginalSlotReach::NoRocketSlot { .. }))
        .map(|slot| slot.record.as_str())
        .collect();
    unselectable.sort_unstable();
    assert_eq!(
        unselectable,
        ["wep_04", "wep_24", "wep_25", "wep_26", "wep_27"]
    );

    for slot in &slots {
        if let OriginalSlotReach::RocketSlot { slots, rocket_ids } = &slot.reach {
            assert_eq!(*slots, ORIGINAL_ROCKET_SLOT_COUNT, "{}", slot.record);
            for id in rocket_ids {
                assert_eq!(
                    original_rocket_weapon_record(*id).as_deref(),
                    Some(slot.record.as_str()),
                    "{} is selected by rocket id {id}",
                    slot.record
                );
            }
            assert!(
                !slot.names_claim(T452_R05_SCRIPT_AMMO_USE),
                "{} has a slot, so the script question does not gate it",
                slot.record
            );
        }
    }

    // The records that declare SONIC or FLASH, and only those, measure a
    // pilot incapacitation; each declares the IMPACT_PROXIMITY its ratio is
    // measured over.
    let mut measured = Vec::new();
    for component in &table.rows {
        let statuses = measure_status_effects(component);
        if statuses.is_empty() {
            continue;
        }
        assert!(
            component.area.radius_m.is_some(),
            "{} declares an effect bit without the IMPACT_PROXIMITY the exposure divides by",
            component.record
        );
        for status in statuses {
            assert!(status.names_claim(T452_R05_STATUS_INCAPACITATION));
            assert!(status.resolves_claim(T452_04_STATUS_DURATION));
            assert_eq!(status.runtime_kind, None);
            measured.push((
                component.record.clone(),
                status.key,
                status.duration.facing_scaled,
            ));
        }
    }
    assert_eq!(
        measured,
        [
            ("wep_08".to_owned(), "SONIC", false),
            ("wep_09".to_owned(), "FLASH", true),
            ("wep_15".to_owned(), "FLASH", true),
        ]
    );
    // The claim keeps its documented meaning in the T452.04 table too: the row
    // is still an effect row whose kind is unstated there.
    let wep08 = table.row("wep_08").expect("wep_08 is a component");
    assert!(
        wep08
            .unknowns
            .iter()
            .any(|unknown| unknown.claim == T452_04_STATUS_DURATION)
    );
}

// ------------------------------------------- the committed static rows ---

const TABLE: &str = include_str!("../../../data/owner-static/engine-facts.toml");

/// `(name, va, bytes)` of every row of one committed `[[<section>]]` array.
fn committed_rows(section: &str, prefix: &str) -> Vec<(String, u64, Vec<u8>)> {
    let marker = format!("[[{section}]]");
    let field = |chunk: &str, name: &str| {
        chunk
            .lines()
            .find_map(|line| {
                let rest = line.strip_prefix(name)?.trim_start().strip_prefix('=')?;
                Some(rest.trim().trim_matches('"').to_string())
            })
            .unwrap_or_else(|| panic!("a {section} row lacks {name}"))
    };
    TABLE
        .split(marker.as_str())
        .skip(1)
        .map(|chunk| chunk.split("\n[").next().unwrap_or(chunk))
        .filter(|chunk| field(chunk, "name").starts_with(prefix))
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

/// The committed rows carry the addresses and bytes of every measured site.
#[test]
fn accept_t452_r05_the_committed_rows_cover_the_measured_sites() {
    let slots = committed_rows("ordnance_slot_fact", "slot.");
    let names: Vec<&str> = slots.iter().map(|(name, _, _)| name.as_str()).collect();
    assert_eq!(
        names,
        [
            "slot.binder_rocket_array",
            "slot.binder_rocket_ids",
            "slot.binder_store",
            "slot.binder_bound",
            "slot.id_range",
            "slot.preset_ids",
            "slot.install_slot_count",
            "slot.script_one_slot",
            "slot.script_all_rocket_slots",
        ]
    );
    let exposure = committed_rows("exposure_fact", "exposure.");
    let names: Vec<&str> = exposure.iter().map(|(name, _, _)| name.as_str()).collect();
    assert_eq!(
        names,
        [
            "exposure.sonic_flag",
            "exposure.flash_flag",
            "exposure.per_hit_test",
            "exposure.victim_gate",
            "exposure.ratio",
            "exposure.falloff",
            "exposure.duration",
            "exposure.radius_parse",
            "exposure.radius_square",
            "exposure.player_branch",
            "exposure.actor_gate",
            "exposure.actor_mode",
            "exposure.recovery",
        ]
    );
    // Spot-check the bytes that carry the two facts the task is about.
    let get = |rows: &[(String, u64, Vec<u8>)], name: &str| {
        rows.iter()
            .find(|(row, _, _)| row == name)
            .map(|(_, va, bytes)| (*va, bytes.clone()))
            .unwrap_or_else(|| panic!("{name} is committed"))
    };
    let (va, bytes) = get(&slots, "slot.id_range");
    assert_eq!(va, 0x0044_40f4);
    assert_eq!(bytes, [0x83, 0xc8, 0xff, 0x83, 0xf9, 0x0a, 0x77, 0x48]);
    let (va, bytes) = get(&exposure, "exposure.per_hit_test");
    assert_eq!(va, 0x004b_9d84);
    assert_eq!(bytes.len(), 17);
    assert_eq!(&bytes[8..13], [0xf6, 0xc5, 0x0a, 0x0f, 0x85]);
}

/// The committed rows match the image byte for byte, so the table cannot
/// silently drift from the image it cites.
#[test]
#[ignore = "requires CS_ENGINE_IMAGE"]
fn accept_t452_r05_the_committed_rows_match_the_engine_image() {
    let image = image();
    let rows = committed_rows("ordnance_slot_fact", "slot.");
    assert_eq!(rows.len(), 9);
    let exposure = committed_rows("exposure_fact", "exposure.");
    assert_eq!(exposure.len(), 13);
    for (name, va, bytes) in rows.into_iter().chain(exposure) {
        assert_eq!(
            at(&image, va, bytes.len()),
            bytes.as_slice(),
            "{name} at 0x{va:x}"
        );
    }
}
