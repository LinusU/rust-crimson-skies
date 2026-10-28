//! Task #2161 (T452-R.03): the measured hardpoint price — what the purchase
//! surface charges for a rocket box — and the Instant Action gate table's
//! unread second column.
//!
//! Task test prefix: `accept_t452_r03_`. Finding:
//! `docs/findings/2026-10-28-t452-r03-hardpoint-price-and-gate-table.md`.

use cs_content::ordnance::{
    ORIGINAL_HARDPOINT_BOX_COUNT_OFFSETS, ORIGINAL_HARDPOINT_COST_PER_BOX,
    ORIGINAL_HARDPOINT_POINT_CALLBACK_VA, ORIGINAL_HARDPOINT_PRICE_VA,
    ORIGINAL_HARDPOINT_ROW_CALLBACK_VA, ORIGINAL_HARDPOINT_WEIGHT_PER_BOX,
    ORIGINAL_PLANE_COST_TOTAL_VA, ORIGINAL_PLANE_RECORD_STRIDE, ORIGINAL_PLANE_RECORD_TABLE_VA,
    ORIGINAL_PLANE_WEIGHT_TOTAL_VA, ORIGINAL_ROCKET_GATE_AUX, ORIGINAL_ROCKET_GATE_TABLE_VA,
    ORIGINAL_ROCKET_GATE_THRESHOLDS, ORIGINAL_ROCKET_ID_WEAPON_INDEX,
    ORIGINAL_ROCKET_SLOT_NAME_CALLBACK_VA, original_hardpoint_price,
};

const IMAGE_BASE: u64 = 0x40_0000;

#[test]
fn accept_t452_r03_the_measured_price_is_per_box_never_per_type() {
    let price = original_hardpoint_price();
    assert_eq!(
        (price.weight_per_box, price.cost_per_box),
        (
            ORIGINAL_HARDPOINT_WEIGHT_PER_BOX,
            ORIGINAL_HARDPOINT_COST_PER_BOX
        )
    );
    assert_eq!((480, 410), (price.weight_per_box, price.cost_per_box));
    assert_eq!(
        price.box_count_offsets,
        ORIGINAL_HARDPOINT_BOX_COUNT_OFFSETS
    );
    assert_eq!(
        (0x34, 0x38),
        (price.box_count_offsets[0], price.box_count_offsets[1])
    );
    assert_eq!((price.sides, price.boxes_per_side), (2, 4));
    assert_eq!(price.weight(0), 0);
    assert_eq!(price.weight(4), 1920);
    assert_eq!(price.cost(4), 1640);
}

#[test]
fn accept_t452_r03_gate_thresholds_match_the_committed_rocket_facts() {
    // The `[[ordinance_rocket_entry]]` rows of engine-facts.toml pin the same
    // table the constants describe: cross-check the two so neither can drift.
    let table = include_str!("../../../data/owner-static/engine-facts.toml");
    let mut rows: Vec<(i64, i64)> = Vec::new();
    let mut in_rows = false;
    for line in table.lines() {
        let line = line.trim();
        if line == "[[ordinance_rocket_entry]]" {
            in_rows = true;
            rows.push((0, 0));
            continue;
        }
        if line.starts_with("[[") {
            in_rows = false;
        }
        if in_rows && line.starts_with("first = ") {
            rows.last_mut().expect("a row").0 = line[8..].parse().expect("int");
        }
        if in_rows && line.starts_with("aux = ") {
            rows.last_mut().expect("a row").1 = line[6..].parse().expect("int");
        }
    }
    assert_eq!(
        rows.len(),
        ORIGINAL_ROCKET_GATE_THRESHOLDS.len(),
        "twelve gate-table rows"
    );
    for (index, (first, _)) in rows.iter().enumerate() {
        assert_eq!(
            u32::try_from(*first).expect("a positive threshold"),
            ORIGINAL_ROCKET_GATE_THRESHOLDS[index],
            "row {index} first"
        );
    }
    for (index, (_, aux)) in rows.iter().take(11).enumerate() {
        assert_eq!(
            u32::try_from(*aux).expect("a non-negative aux"),
            ORIGINAL_ROCKET_GATE_AUX[index],
            "row {index} aux"
        );
    }
    // Row order is rocket-id order: every selectable id's threshold is the
    // same table index ORIGINAL_ROCKET_ID_WEAPON_INDEX resolves it under.
    for (id, threshold) in ORIGINAL_ROCKET_GATE_THRESHOLDS.iter().enumerate().take(11) {
        assert!(
            ORIGINAL_ROCKET_ID_WEAPON_INDEX[id] >= 5,
            "id {id} (threshold {threshold}) maps to a selectable record"
        );
    }
    assert_eq!(ORIGINAL_ROCKET_GATE_THRESHOLDS[11], 1, "the none row");
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

/// The `[[hardpoint_price_fact]]` rows of `engine-facts.toml`: `(name, va,
/// bytes)`, scanned — the subset format is line-oriented.
fn price_facts() -> Vec<(String, u64, Vec<u8>)> {
    let table = include_str!("../../../data/owner-static/engine-facts.toml");
    let mut facts = Vec::new();
    let mut current: Option<(String, u64, Vec<u8>)> = None;
    for line in table.lines() {
        let line = line.trim();
        if line == "[[hardpoint_price_fact]]" {
            if let Some(fact) = current.take() {
                facts.push(fact);
            }
            current = Some((String::new(), 0, Vec::new()));
            continue;
        }
        if line.starts_with("[[") {
            if let Some(fact) = current.take() {
                facts.push(fact);
            }
            continue;
        }
        let Some((name, va, bytes)) = current.as_mut() else {
            continue;
        };
        if let Some(value) = line.strip_prefix("name = \"") {
            *name = value.trim_end_matches('"').to_owned();
        } else if let Some(value) = line.strip_prefix("va = 0x") {
            *va = u64::from_str_radix(value, 16).expect("a hex va");
        } else if let Some(value) = line.strip_prefix("bytes = \"") {
            let value = value.trim_end_matches('"');
            *bytes = (0..value.len())
                .step_by(2)
                .map(|position| {
                    u8::from_str_radix(&value[position..position + 2], 16).expect("a byte")
                })
                .collect();
        }
    }
    if let Some(fact) = current.take() {
        facts.push(fact);
    }
    facts
}

#[test]
#[ignore = "requires CS_ENGINE_IMAGE"]
fn accept_t452_r03_image_bytes_are_the_committed_facts() {
    let image = image();
    let facts = price_facts();
    assert_eq!(facts.len(), 5, "every committed hardpoint price row");
    for (name, va, bytes) in &facts {
        assert_eq!(
            at(&image, *va, bytes.len()),
            bytes.as_slice(),
            "{name} at {va:#x} does not match the committed bytes"
        );
    }
    // The price instructions the facts encode, decomposed.
    assert_eq!(480 - 70, ORIGINAL_HARDPOINT_COST_PER_BOX);
    assert_eq!(0x1e0_u32, ORIGINAL_HARDPOINT_WEIGHT_PER_BOX);
    // The weight/cost row handler's E 2259 count reads both box cells.
    assert_eq!(ORIGINAL_HARDPOINT_ROW_CALLBACK_VA, 0x40_b2b4);
    // The cited window and totalizers pin the facts' addresses.
    let va_of = |name: &str| {
        facts
            .iter()
            .find(|(n, _, _)| n == name)
            .map(|(_, va, _)| *va)
            .expect(name)
    };
    assert_eq!(
        va_of("hardpoint_price.weight_and_cost"),
        ORIGINAL_HARDPOINT_PRICE_VA
    );
    assert_eq!(
        va_of("hardpoint_price.row_count"),
        ORIGINAL_HARDPOINT_ROW_CALLBACK_VA + 6
    );
    assert!(
        (ORIGINAL_PLANE_WEIGHT_TOTAL_VA..ORIGINAL_PLANE_COST_TOTAL_VA)
            .contains(&va_of("hardpoint_price.weight_total")),
        "the weight-total fact sits inside 0x405550"
    );
    assert!(
        va_of("hardpoint_price.cost_total") > ORIGINAL_PLANE_COST_TOTAL_VA
            && va_of("hardpoint_price.cost_total") < ORIGINAL_PLANE_COST_TOTAL_VA + 0x100,
        "the cost-total fact sits inside 0x405680"
    );
}

#[test]
#[ignore = "requires CS_ENGINE_IMAGE"]
fn accept_t452_r03_image_pins_the_box_count_consumers() {
    let image = image();
    // E 2031 reads the slot's rocket id at record + 0xa8, then answers -1
    // unless the slot index is below the side's box count: slots 0..4 compare
    // record + 0x34, slots 4..8 compare record + 0x38 + 4.
    assert_eq!(
        at(&image, ORIGINAL_ROCKET_SLOT_NAME_CALLBACK_VA + 0x1d, 7),
        [
            0x8b, 0x9c, 0xbe, 0xa8, 0x00, 0x00, 0x00, // mov ebx, [esi+edi*4+0xa8]
        ]
    );
    assert_eq!(
        at(&image, ORIGINAL_ROCKET_SLOT_NAME_CALLBACK_VA + 0x4a, 8),
        [
            0x83, 0xff, 0x04, // cmp edi, 4
            0x7d, 0x19, // jge (side 1)
            0x3b, 0x7e, 0x34, // cmp edi, [esi+0x34]
        ]
    );
    assert_eq!(
        at(&image, ORIGINAL_ROCKET_SLOT_NAME_CALLBACK_VA + 0x68, 10),
        [
            0x8b, 0x56, 0x38, // mov edx, [esi+0x38]
            0x83, 0xc2, 0x04, // add edx, 4
            0x3b, 0xfa, // cmp edi, edx
            0x0f, 0x8c, // jl
        ]
    );
    // E 2245's set fills the side's four draft rocket slots at
    // 0x64cc20 + 16 * point (the draft record's +0xa8 array).
    assert_eq!(
        at(&image, ORIGINAL_HARDPOINT_POINT_CALLBACK_VA + 0x40, 18),
        [
            0x8b, 0x45, 0x08, // mov eax, [ebp+8] (point)
            0xc1, 0xe0, 0x02, // shl eax, 2
            0x8d, 0x3c, 0x85, 0x20, 0xcc, 0x64, 0x00, // lea edi, [eax*4+0x64cc20]
            0xbe, 0x04, 0x00, 0x00, 0x00, // mov esi, 4
        ]
    );
    // The gate table's first row, and the plane-record table base that the
    // twelfth row's second dword overlaps.
    assert_eq!(
        at(&image, ORIGINAL_ROCKET_GATE_TABLE_VA, 8),
        [0x01, 0x00, 0x00, 0x00, 0x04, 0x00, 0x00, 0x00]
    );
    assert_eq!(
        ORIGINAL_ROCKET_GATE_TABLE_VA + 8 * 11 + 4,
        ORIGINAL_PLANE_RECORD_TABLE_VA,
        "row 11's would-be second dword is the plane-record table base"
    );
    assert_eq!(ORIGINAL_PLANE_RECORD_STRIDE, 0x33 * 4);
}
