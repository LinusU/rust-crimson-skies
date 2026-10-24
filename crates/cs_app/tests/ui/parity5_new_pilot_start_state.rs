//! Acceptance task #2074 (P5-FEED-A): a new pilot's starting cash and plane
//! record, read from the engine image and committed in
//! `data/owner-static/engine-facts.toml`. Task test prefix:
//! `accept_parity5_feed_start_state_`.
//!
//! * The first tests run the production accessor over the committed table;
//!   they run in CI and prove the table's shape and the arithmetic built on
//!   it, not retail behaviour.
//! * The image test re-reads the cited bytes of `$CS_ENGINE_IMAGE`; it is
//!   `#[ignore]`d because CI has no image.

use cs_app::engine_facts::engine_facts;

const RECORD_DWORDS: usize = 51;

fn template_record() -> [i32; RECORD_DWORDS] {
    let new_pilot = engine_facts().expect("committed table").new_pilot();
    let mut record = [0_i32; RECORD_DWORDS];
    for word in new_pilot.template_words() {
        record[(word.offset / 4) as usize] = word.value;
    }
    record
}

#[test]
fn accept_parity5_feed_start_state_a_the_cash_is_zero_unless_the_profile_grants() {
    let new_pilot = engine_facts().expect("committed table").new_pilot();
    assert_eq!(new_pilot.starting_cash(false), 0);
    assert_eq!(new_pilot.starting_cash(true), 250_000);
    // The cash cell is the one the Plane Construction script's cost test
    // reads, and the grant is gated by the cell the Ordinance table reads.
    let facts = engine_facts().expect("committed table");
    assert_eq!(new_pilot.value("cash_va"), 0x64b788);
    assert_eq!(
        new_pilot.value("cash_grant_gate_va"),
        facts.ordinance().value("flag_a_va")
    );
}

#[test]
fn accept_parity5_feed_start_state_b_the_template_is_the_devastator_loadout() {
    let new_pilot = engine_facts().expect("committed table").new_pilot();
    let record = template_record();
    // Occupied, airframe 5.
    assert_eq!(new_pilot.template_word(0), Some(1));
    assert_eq!(new_pilot.template_word(0x2c), Some(5));
    // Guns 2, 1, 0 and the empty position; ammunition 0, 0, 0, 4; the eight
    // rocket boxes 1, 1, 11, 11, 1, 1, 11, 11; two low and two high boxes.
    assert_eq!(&record[0x88 / 4..0x98 / 4], [2, 1, 0, 5]);
    assert_eq!(&record[0x98 / 4..0xa8 / 4], [0, 0, 0, 4]);
    assert_eq!(&record[0xa8 / 4..0xc8 / 4], [1, 1, 11, 11, 1, 1, 11, 11]);
    assert_eq!(
        (new_pilot.template_word(0x34), new_pilot.template_word(0x38)),
        (Some(2), Some(2))
    );
    assert_eq!(new_pilot.template_word(0x84), Some(7), "guns 0..=2 present");
    // Measured as unknown, so absent rather than guessed.
    for unknown in [0x4, 0x68, 0x6c, 0x70, 0xc8] {
        assert_eq!(new_pilot.template_word(unknown), None, "+{unknown:#x}");
    }
}

#[test]
fn accept_parity5_feed_start_state_c_an_unchecked_default_box_empties_the_plane() {
    let new_pilot = engine_facts().expect("committed table").new_pilot();
    let mut record = template_record();
    new_pilot.apply_empty_loadout(&mut record);
    assert_eq!(&record[0x88 / 4..0x98 / 4], [5, 5, 5, 5], "no guns");
    // The store order leaves ammunition 0 at 5, the rest at 4.
    assert_eq!(&record[0x98 / 4..0xa8 / 4], [5, 4, 4, 4]);
    assert_eq!(&record[0xa8 / 4..0xc8 / 4], [11; 8], "no rockets");
    assert_eq!(record[0x84 / 4] & 0x0f, 0, "no gun bits");
    assert_eq!(record[0x30 / 4], 6);
    assert_eq!((record[0x34 / 4], record[0x38 / 4]), (0, 0), "no boxes");
    // The airframe, name slot and paint are not touched.
    assert_eq!(record[0x2c / 4], 5);
    assert_eq!(record[0x44 / 4], template_record()[0x44 / 4]);
}

#[test]
#[ignore = "requires CS_ENGINE_IMAGE"]
fn accept_parity5_feed_start_state_image_the_cited_bytes_match_the_image() {
    use cs_content::coordinates::{load_engine_image, original_image_digest};
    let loaded = load_engine_image().expect("CS_ENGINE_IMAGE names the measured image");
    assert_eq!(loaded.digest, original_image_digest());
    let image = loaded.bytes.as_slice();
    let facts = engine_facts().expect("committed table");
    let new_pilot = facts.new_pilot();
    let at = |va: u64| usize::try_from(facts.image_offset(va).expect("inside")).expect("offset");
    let bytes = |va: u64, len: usize| &image[at(va)..at(va) + len];
    let dword = |va: u64| i32::from_le_bytes(bytes(va, 4).try_into().expect("four bytes"));
    let le = |value: u64| (value as u32).to_le_bytes();

    // Cash: `mov [cash], ebx` (ebx = 0) and `mov dword [cash], imm32`, the
    // latter behind `cmp [gate], ebx`.
    let cash = new_pilot.value("cash_va");
    assert_eq!(
        bytes(new_pilot.value("cash_reset_write_va"), 6),
        [&[0x89, 0x1d][..], &le(cash)].concat()
    );
    assert_eq!(
        bytes(new_pilot.value("cash_grant_write_va"), 10),
        [
            &[0xc7, 0x05][..],
            &le(cash),
            &le(new_pilot.value("cash_grant_value"))
        ]
        .concat()
    );
    assert_eq!(
        bytes(new_pilot.value("cash_grant_test_va"), 6),
        [
            &[0x39, 0x1d][..],
            &le(new_pilot.value("cash_grant_gate_va"))
        ]
        .concat()
    );
    // The profile table binds the name `nPlayerCash` to the cash cell.
    assert_eq!(
        bytes(new_pilot.value("cash_profile_name_va"), 12),
        *b"nPlayerCash\0"
    );
    assert_eq!(
        bytes(new_pilot.value("cash_profile_bind_va"), 5),
        [&[0x68][..], &le(cash)].concat()
    );

    // The template record.
    for word in new_pilot.template_words() {
        assert_eq!(dword(word.va), word.value, "+{:#x}", word.offset);
    }

    // The empty-loadout stores: each names `working plane + offset` in its
    // operand, except the rocket fill, a `rep stosd` from `mov edi, imm32`.
    let plane = new_pilot.value("working_plane_va");
    for store in new_pilot.empty_loadout_stores() {
        if (0xa8..0xc8).contains(&store.offset) {
            assert_eq!(bytes(store.va, 2), [0xf3, 0xab], "rep stosd");
            continue;
        }
        // `mov [mem], eax` has the short form `a3 imm32`.
        let operand = if bytes(store.va, 1) == [0xa3] { 1 } else { 2 };
        assert_eq!(
            bytes(store.va + operand, 4),
            le(plane + u64::from(store.offset)),
            "+{:#x}",
            store.offset
        );
    }
    assert_eq!(bytes(0x40abe3, 5), [&[0xb8][..], &le(11)].concat());
    assert_eq!(
        bytes(0x40abe8, 5),
        [&[0xbf][..], &le(plane + 0xa8)].concat()
    );
    assert_eq!(
        bytes(new_pilot.value("empty_gun_bits_mask_va"), 7),
        [
            &[0x80, 0x25][..],
            &le(plane + new_pilot.value("gun_bits_offset")),
            &[new_pilot.value("empty_gun_bits_mask") as u8]
        ]
        .concat()
    );
}
