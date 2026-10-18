//! M01-LC-FC-B (#1670): the Flight Check's gun and rocket lists, measured
//! from the decrypted engine image, and the fresh-campaign defaults of the
//! pilot's and the wingman's plane (finding
//! `docs/findings/2026-10-19-m01-lc-fc-b-flight-check-gun-and-rocket-lists.md`).
//!
//! Two things are pinned:
//!
//! * the committed `[flight_check_layout]` / `[[flight_check_default]]`
//!   tables parse under the production reader and derive the lists the
//!   image's handler builds for the campaign's Devastator;
//! * against the owner's decrypted image, every cited byte (the dispatcher,
//!   its jump table, the handlers' constants, the roster pointer and the two
//!   roster records) is read back out of the image itself, so the committed
//!   rows cannot silently drift.

use cs_app::engine_facts::{
    FlightCheckGunLine, FlightCheckPlaneSlot, FlightCheckRocketLine, MissionAirframe, engine_facts,
};
use cs_app::mission_start::CAMPAIGN_AIRFRAME_ROW;
use cs_content::coordinates::{load_engine_image, original_image_digest};

const SLOTS: [FlightCheckPlaneSlot; 2] =
    [FlightCheckPlaneSlot::Pilot, FlightCheckPlaneSlot::Wingman];

fn gun(number: u32, gun: u32, ammo: u32) -> Option<FlightCheckGunLine> {
    Some(FlightCheckGunLine { number, gun, ammo })
}

fn rocket(number: u32, rocket: u32) -> Option<FlightCheckRocketLine> {
    Some(FlightCheckRocketLine { number, rocket })
}

#[test]
fn accept_m01_lc_fc_b_the_committed_defaults_derive_the_devastators_lists() {
    let facts = engine_facts().expect("the committed table parses");
    let flight_check = facts.flight_check();
    let layout = flight_check.layout();
    assert!(layout.claim.starts_with("OWNER-STATIC-"));
    assert_eq!(layout.value("list_items"), 8);

    // Both slots start from the campaign's Devastator record, the one M01's
    // player airframe row already cites (`0x61a81c`).
    let m01 = facts.mission("zbd/c3/m01").expect("M01's row");
    let MissionAirframe::Known { va, .. } = &m01.airframe else {
        panic!("M01's airframe is committed");
    };
    let pilot = flight_check.default_for(FlightCheckPlaneSlot::Pilot);
    let wingman = flight_check.default_for(FlightCheckPlaneSlot::Wingman);
    assert_eq!(pilot.va, *va);
    assert_eq!(pilot.plane_id as usize, CAMPAIGN_AIRFRAME_ROW);
    assert_eq!(wingman.plane_id as usize, CAMPAIGN_AIRFRAME_ROW);
    assert_eq!(wingman.va, pilot.va + layout.value("record_stride"));

    for slot in SLOTS {
        // Three guns, each shown twice (their pair-mask bits are set); the
        // fourth slot holds the no-gun id and its two lines are empty.
        assert_eq!(
            flight_check.gun_list(slot),
            vec![
                gun(1, 2, 0),
                gun(2, 2, 0),
                gun(3, 1, 0),
                gun(4, 1, 0),
                gun(5, 0, 0),
                gun(6, 0, 0),
                None,
                None,
            ],
            "{slot:?} guns"
        );
        // Rocket slots 0, 1, 4 and 5 hold rocket 1; the others none.
        assert_eq!(
            flight_check.rocket_list(slot),
            vec![
                rocket(1, 1),
                rocket(2, 1),
                None,
                None,
                rocket(5, 1),
                rocket(6, 1),
                None,
                None,
            ],
            "{slot:?} rockets"
        );
    }
}

/// With the owner's image named, every fact the rows commit is read back out
/// of the image's own bytes — the drift guard.
#[test]
#[ignore = "requires CS_ENGINE_IMAGE"]
fn accept_m01_lc_fc_b_image_rows_match_the_image() {
    let loaded = load_engine_image().expect("CS_ENGINE_IMAGE names the measured image");
    assert_eq!(loaded.digest, original_image_digest());
    let image = loaded.bytes.as_slice();
    let facts = engine_facts().expect("the committed table parses");
    let layout = facts.flight_check().layout();
    let at = |va: u64| -> usize {
        usize::try_from(facts.image_offset(va).expect("the va is inside the image"))
            .expect("an offset")
    };
    let bytes = |va: u64, expected: &[u8]| {
        assert_eq!(
            &image[at(va)..at(va) + expected.len()],
            expected,
            "bytes at 0x{va:x}"
        );
    };
    let u32_at = |va: u64| -> u32 {
        u32::from_le_bytes(image[at(va)..at(va) + 4].try_into().expect("four bytes"))
    };
    let le = |value: u64| (value as u32).to_le_bytes();

    // The dispatcher: `lea ecx,[eax-0x7d0]; cmp ecx,0x26; ja ..; jmp [ecx*4+table]`.
    let table = layout.value("jump_table_va");
    bytes(
        0x4093e2,
        &[0x8d, 0x88, 0x30, 0xf8, 0xff, 0xff, 0x83, 0xf9, 0x26],
    );
    assert_eq!(layout.value("jump_table_base_id"), 2000);
    assert_eq!(layout.value("jump_table_entries"), 0x26 + 1);
    bytes(0x4093f1, &[0xff, 0x24, 0x8d]);
    bytes(0x4093f4, &le(table));
    let entry = |id: u64| {
        u64::from(u32_at(
            table + 4 * (id - layout.value("jump_table_base_id")),
        ))
    };
    assert_eq!(entry(2005), layout.value("gun_list_va"));
    assert_eq!(entry(2007), layout.value("gun_list_va"));
    assert_eq!(entry(2006), layout.value("rocket_list_va"));
    assert_eq!(entry(2008), layout.value("rocket_list_va"));

    // Each list handler answers the item count 8 for index -1 and picks the
    // slot by id (`cmp eax,0x7d5` / `0x7d6`).
    let gun_list = layout.value("gun_list_va");
    let rocket_list = layout.value("rocket_list_va");
    let count = {
        let mut store = vec![0xc7, 0x03];
        store.extend_from_slice(&le(layout.value("list_items")));
        store
    };
    bytes(gun_list + 7, &count);
    bytes(rocket_list + 7, &count);
    bytes(gun_list + 0x1b, &[0x3d, 0xd5, 0x07, 0x00, 0x00]);
    bytes(rocket_list + 0x1b, &[0x3d, 0xd6, 0x07, 0x00, 0x00]);

    // Item numbering string, the no-gun / no-rocket sentinels and the three
    // label bases.
    bytes(0x409c63, &[0x68, 0x07, 0x02, 0x00, 0x00]);
    assert_eq!(layout.value("item_number_string"), 0x207);
    bytes(0x409c81, &[0x83, 0xff, layout.value("gun_none") as u8]);
    bytes(0x409d63, &[0x83, 0xf8, layout.value("rocket_none") as u8]);
    // gun: 0xcee + (2 & 7 != 1 ? 10 : 0); ammo: 0xd25; rocket: 0xd34 + 15.
    bytes(0x40fd43, &[0x83, 0xe0, 0x0a, 0x05, 0xee, 0x0c, 0x00, 0x00]);
    assert_eq!(layout.value("gun_label_base"), 0xcee + 0x0a);
    bytes(0x40fd78, &[0xb9, 0x25, 0x0d, 0x00, 0x00]);
    assert_eq!(layout.value("ammo_label_base"), 0xd25);
    bytes(0x40fdcd, &[0x83, 0xe0, 0x0f, 0x05, 0x34, 0x0d, 0x00, 0x00]);
    assert_eq!(layout.value("rocket_label_base"), 0xd34 + 0x0f);

    // The record array: the allocation loop is bounded at `record_count`.
    bytes(0x40608c, &[0x83, 0xf9, layout.value("record_count") as u8]);
    assert_eq!(layout.value("record_array_va"), 0x64b78c);
    assert_eq!(layout.value("record_stride"), 0xcc);

    // Roster init: the roster pointer store, the two selection cells reset
    // to -3 and the two default cells set to 0 and 1.
    let mut store = vec![0xc7, 0x05];
    store.extend_from_slice(&le(0x6480c8));
    store.extend_from_slice(&le(layout.value("roster_va")));
    bytes(layout.value("roster_pointer_va"), &store);
    bytes(0x4113bd, &{
        let mut cells = vec![0xa3];
        cells.extend_from_slice(&le(layout.value("selection_cells_va")));
        cells
    });
    bytes(0x4113c2, &{
        let mut cells = vec![0xa3];
        cells.extend_from_slice(&le(layout.value("selection_cells_va") + 4));
        cells
    });
    bytes(0x4113b1, &[0xb8, 0xfd, 0xff, 0xff, 0xff]);
    let default_cells = layout.value("default_cells_va");
    bytes(0x411477, &{
        let mut cell = vec![0x89, 0x1d];
        cell.extend_from_slice(&le(default_cells));
        cell
    });
    bytes(0x41147d, &{
        let mut cell = vec![0xc7, 0x05];
        cell.extend_from_slice(&le(default_cells + 4));
        cell.extend_from_slice(&le(1));
        cell
    });

    // The copies into live records 0 and 1: `mov r,[roster]; lea esi,[r+off]`
    // with the roster record's offset, then `mov edi,<live record>`.
    let stride = layout.value("record_stride");
    let array = layout.value("record_array_va");
    let copy = |va: u64, modrm: u8, record: u64, live: u64| {
        bytes(va + 6, &[0x8d, modrm]);
        bytes(va + 8, &le(record * stride));
        bytes(va + 0x13, &[0xbf]);
        bytes(va + 0x14, &le(live));
    };
    copy(
        layout.value("pilot_copy_va"),
        0xb1,
        layout.value("pilot_roster_record"),
        array,
    );
    copy(
        layout.value("wingman_copy_va"),
        0xb2,
        layout.value("wingman_roster_record"),
        array + stride,
    );

    // The roster records themselves.
    for (slot, record) in [
        (
            FlightCheckPlaneSlot::Pilot,
            layout.value("pilot_roster_record"),
        ),
        (
            FlightCheckPlaneSlot::Wingman,
            layout.value("wingman_roster_record"),
        ),
    ] {
        let row = facts.flight_check().default_for(slot);
        assert_eq!(
            row.va,
            layout.value("roster_va") + record * stride,
            "{slot:?}"
        );
        let field = |offset: u64| u32_at(row.va + offset);
        assert_eq!(field(layout.value("plane_id_offset")), row.plane_id);
        assert_eq!(
            image[at(row.va + layout.value("pair_mask_offset"))],
            row.pair_mask as u8
        );
        for (index, gun) in row.guns.iter().enumerate() {
            assert_eq!(field(layout.value("gun_offset") + 4 * index as u64), *gun);
        }
        for (index, ammo) in row.ammo.iter().enumerate() {
            assert_eq!(field(layout.value("ammo_offset") + 4 * index as u64), *ammo);
        }
        for (index, rocket) in row.rockets.iter().enumerate() {
            assert_eq!(
                field(layout.value("rocket_offset") + 4 * index as u64),
                *rocket
            );
        }
    }
}
