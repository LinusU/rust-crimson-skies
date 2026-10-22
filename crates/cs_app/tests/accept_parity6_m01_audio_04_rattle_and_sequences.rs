//! Acceptance for Rally #1918 `PARITY6-M01-AUDIO-TIMELINE.04`: the keyed voice-slot
//! helper `0x45e470` behind the rattle play, the vehicle step that runs it, the
//! `SOUND ... AT_NODE` sequence handler `0x4ef5d0`, the `START_TIME` clocks and the
//! `EXECUTION_BY_RANGE` gate (finding
//! `docs/findings/2026-10-23-m01-audio-rattle-slot-and-sound-sequences.md`).
//!
//! The committed rows are parsed by the production reader; the ignored test
//! re-reads the owner's decrypted image and requires every cited byte to match.
//! Task test prefix: `accept_parity6_m01_audio_04_`.

use cs_app::engine_facts::engine_facts;

const TABLE: &str = include_str!("../../../data/owner-static/engine-facts.toml");
const IMAGE_BASE: usize = 0x40_0000;
const CLAIM: &str = "OWNER-STATIC-2026-10-22";

/// The `[[audio_fact]]` rows this task added.
const NEW_FACT_ROWS: [&str; 21] = [
    "rattle.slot_helper",
    "rattle.slot_update_and_expiry",
    "rattle.slot_reap",
    "rattle.slot_tick",
    "rattle.slot_tick_caller",
    "rattle.slot_ids",
    "rattle.call_args",
    "vehicle.update_dispatch",
    "vehicle.update_table",
    "vehicle.update_loop",
    "vehicle.update_callback",
    "sequence.sound_compile",
    "sequence.sound_at_node",
    "sequence.start_time_modes",
    "sequence.start_time_mode_2",
    "sequence.start_time_mode_3",
    "sequence.step_clocks",
    "animation.clock_advance",
    "animation.range_gate",
    "animation.range_parse_one_value",
    "animation.range_square",
];

/// One `[[audio_fact]]` element.
struct Row {
    fields: Vec<(String, String)>,
}

impl Row {
    fn get(&self, key: &str) -> &str {
        self.fields
            .iter()
            .find(|(name, _)| name == key)
            .map(|(_, value)| value.as_str())
            .unwrap_or_else(|| panic!("row lacks {key}"))
    }

    fn va(&self) -> usize {
        let text = self.get("va");
        usize::from_str_radix(text.trim_start_matches("0x"), 16).expect("hex va")
    }

    fn bytes(&self) -> Vec<u8> {
        let hex = self.get("bytes");
        (0..hex.len() / 2)
            .map(|i| u8::from_str_radix(&hex[2 * i..2 * i + 2], 16).expect("hex byte"))
            .collect()
    }
}

fn facts() -> Vec<Row> {
    let mut out = Vec::new();
    let mut current: Option<Row> = None;
    for line in TABLE.lines() {
        if line.starts_with('[') {
            out.extend(current.take());
            if line.trim() == "[[audio_fact]]" {
                current = Some(Row { fields: Vec::new() });
            }
        } else if let Some(row) = current.as_mut()
            && let Some((key, value)) = line.split_once(" = ")
        {
            row.fields.push((
                key.trim().to_string(),
                value.trim().trim_matches('"').to_string(),
            ));
        }
    }
    out.extend(current);
    out
}

fn new_rows() -> Vec<Row> {
    facts()
        .into_iter()
        .filter(|row| NEW_FACT_ROWS.contains(&row.get("name")))
        .collect()
}

#[test]
fn accept_parity6_m01_audio_04_the_table_parses_and_every_new_row_is_cited() {
    engine_facts().expect("the committed table parses");
    let all = facts();
    for name in NEW_FACT_ROWS {
        let rows: Vec<&Row> = all.iter().filter(|row| row.get("name") == name).collect();
        assert_eq!(rows.len(), 1, "{name} is exactly one row");
        let row = rows[0];
        assert_eq!(row.get("claim"), CLAIM, "{name}");
        assert_eq!(row.get("provenance"), "static", "{name}");
        assert!(!row.bytes().is_empty(), "{name} cites no bytes");
        assert!(row.get("fact").len() > 30, "{name} states no fact");
    }
}

#[test]
fn accept_parity6_m01_audio_04_the_rows_cover_the_three_questions() {
    let rows = new_rows();
    let fact = |name: &str| {
        rows.iter()
            .find(|row| row.get("name") == name)
            .unwrap_or_else(|| panic!("{name}"))
    };
    // (a) the rattle helper: one keyed record, started once, expiry re-stamped
    // with ttl 0, stopped by a strictly-later slot clock.
    assert!(fact("rattle.slot_helper").get("fact").contains("0x593590"));
    assert!(
        fact("rattle.slot_reap")
            .get("fact")
            .contains("strictly greater")
    );
    assert!(fact("rattle.slot_tick").get("fact").contains("second tick"));
    // (b) the SOUND handler is a compile step and the range gate squares the range.
    assert!(
        fact("sequence.sound_compile")
            .get("fact")
            .contains("compile-time")
    );
    assert!(
        fact("animation.range_parse_one_value")
            .get("fact")
            .contains("squares")
    );
    // (c) the three START_TIME modes store the bytes 1, 2 and 3.
    assert_eq!(fact("sequence.start_time_mode_2").bytes()[2], 0x01);
    assert_eq!(fact("sequence.start_time_mode_2").bytes()[3], 0x02);
    assert_eq!(fact("sequence.start_time_mode_3").bytes()[3], 0x03);
}

#[test]
#[ignore = "requires CS_ENGINE_IMAGE"]
fn accept_parity6_m01_audio_04_cited_rows_match_the_engine_image() {
    let path = std::env::var("CS_ENGINE_IMAGE").expect("CS_ENGINE_IMAGE must name the image");
    let image = std::fs::read(path).expect("read the engine image");
    for row in new_rows() {
        let offset = row.va() - IMAGE_BASE;
        let bytes = row.bytes();
        assert_eq!(
            &image[offset..offset + bytes.len()],
            bytes.as_slice(),
            "{} at {:#x}",
            row.get("name"),
            row.va()
        );
    }
    // The jump table of the vehicle step: modes 0 and 4 reach the same handler.
    let table = new_rows()
        .into_iter()
        .find(|row| row.get("name") == "vehicle.update_table")
        .unwrap()
        .bytes();
    let entry = |i: usize| u32::from_le_bytes(table[4 * i..4 * i + 4].try_into().unwrap());
    assert_eq!(entry(0), entry(4), "modes 0 and 4 share a handler");
    assert_eq!(entry(3), entry(5), "modes 3 and 5 share a handler");
}
