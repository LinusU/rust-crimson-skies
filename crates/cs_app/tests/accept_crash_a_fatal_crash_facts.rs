//! Acceptance for Rally #1853 `PARITY4-CRASH-A`: the static trace of the
//! original's fatal-contact removal path, its `Fatal Crash!` message call and
//! its detached crash camera, recorded as cited `[[crash_fact]]` rows of
//! `data/owner-static/engine-facts.toml` (finding
//! `docs/findings/2026-10-22-crash-a-fatal-contact-and-crash-camera.md`).
//!
//! The committed table is parsed by the production reader; the ignored test
//! re-reads every cited instruction from the owner's decrypted image so a row
//! cannot drift from the bytes it cites.

use cs_app::engine_facts::engine_facts;

const TABLE: &str = include_str!("../../../data/owner-static/engine-facts.toml");
const IMAGE_BASE: usize = 0x40_0000;

/// One `[[crash_fact]]` row.
struct Row {
    name: String,
    va: usize,
    bytes: Vec<u8>,
    fact: String,
}

fn field(chunk: &str, name: &str) -> String {
    chunk
        .lines()
        .find_map(|line| {
            let rest = line.strip_prefix(name)?.trim_start().strip_prefix('=')?;
            Some(rest.trim().trim_matches('"').to_string())
        })
        .unwrap_or_else(|| panic!("crash_fact lacks {name}"))
}

fn rows() -> Vec<Row> {
    TABLE
        .split("[[crash_fact]]")
        .skip(1)
        .map(|chunk| {
            // A row ends at the next table header or the end of the file.
            let chunk = chunk.split("\n[").next().unwrap_or(chunk);
            assert_eq!(field(chunk, "claim"), "OWNER-STATIC-2026-10-22");
            assert_eq!(field(chunk, "provenance"), "static");
            let va = field(chunk, "va");
            let va = usize::from_str_radix(va.trim_start_matches("0x"), 16).expect("hex va");
            let hex = field(chunk, "bytes");
            let bytes = (0..hex.len() / 2)
                .map(|i| u8::from_str_radix(&hex[2 * i..2 * i + 2], 16).expect("hex byte"))
                .collect();
            Row {
                name: field(chunk, "name"),
                va,
                bytes,
                fact: field(chunk, "fact"),
            }
        })
        .collect()
}

fn row<'a>(rows: &'a [Row], name: &str) -> &'a Row {
    rows.iter()
        .find(|row| row.name == name)
        .unwrap_or_else(|| panic!("no crash_fact named {name}"))
}

#[test]
fn accept_crash_a_the_committed_table_parses_with_the_crash_rows() {
    let facts = engine_facts().expect("the committed table parses");
    let rows = rows();
    assert_eq!(rows.len(), 18);
    let mut names: Vec<&str> = rows.iter().map(|row| row.name.as_str()).collect();
    names.sort_unstable();
    names.dedup();
    assert_eq!(names.len(), rows.len(), "row names are unique");
    for row in &rows {
        assert!(!row.bytes.is_empty(), "{} cites no bytes", row.name);
        assert!(!row.fact.is_empty(), "{} states no fact", row.name);
        let offset = facts
            .image_offset(row.va as u64)
            .unwrap_or_else(|error| panic!("{}: {error}", row.name));
        assert!(
            offset < 0x30_0000,
            "{} va {:#x} is past the image",
            row.name,
            row.va
        );
    }
}

#[test]
fn accept_crash_a_the_message_camera_and_death_rows_state_the_measured_values() {
    let rows = rows();
    // `Fatal Crash!` is string id 162; `was shot down` is 169.
    assert_eq!(
        row(&rows, "msg.fatal_crash_id").bytes,
        [0x68, 0xa2, 0, 0, 0]
    );
    assert_eq!(
        row(&rows, "msg.was_shot_down_id").bytes,
        [0x68, 0xa9, 0, 0, 0]
    );
    // The message call's second argument: 5.0f for both death messages.
    for name in ["msg.fatal_crash_duration", "msg.shot_down_duration"] {
        assert_eq!(row(&rows, name).bytes[1..], 5.0f32.to_le_bytes(), "{name}");
    }
    assert_eq!(
        row(&rows, "msg.other_call_duration").bytes[1..],
        1.0f32.to_le_bytes()
    );
    // The crash camera is mode 5 and reads crash_horiz (0x44) and crash_y (0x48).
    assert_eq!(&row(&rows, "camera.set_mode5").bytes[8..10], [0x6a, 0x05]);
    assert_eq!(
        &row(&rows, "camera.crash_horiz_x").bytes[3..6],
        [0xd8, 0x48, 0x44]
    );
    assert_eq!(
        &row(&rows, "camera.crash_horiz_z").bytes[3..6],
        [0xd8, 0x48, 0x44]
    );
    assert_eq!(
        &row(&rows, "camera.crash_y").bytes[6..9],
        [0xd8, 0x40, 0x48]
    );
    // The health test is followed by the call to the removal path 0x48b920
    // (the image test checks the call's target).
    assert_eq!(row(&rows, "death.call_removal").bytes[6], 0xe8);
    assert!(row(&rows, "death.health_test").fact.contains("<= 0"));
}

#[test]
#[ignore = "requires CS_ENGINE_IMAGE"]
fn accept_crash_a_cited_rows_match_the_engine_image() {
    let path = std::env::var("CS_ENGINE_IMAGE").expect("CS_ENGINE_IMAGE must name the image");
    let image = std::fs::read(path).expect("read the engine image");
    for row in rows() {
        let offset = row.va - IMAGE_BASE;
        assert_eq!(
            &image[offset..offset + row.bytes.len()],
            row.bytes.as_slice(),
            "{} at {:#x}",
            row.name,
            row.va
        );
    }
    // The removal path call lands on 0x48b920.
    let at = 0x48d7cc - IMAGE_BASE;
    let rel = i32::from_le_bytes(image[at + 1..at + 5].try_into().unwrap());
    assert_eq!(0x48d7cc + 5 + i64::from(rel), 0x48b920);
    // The message call lands on 0x4588e0.
    let at = 0x48b9e9 - IMAGE_BASE;
    let rel = i32::from_le_bytes(image[at + 1..at + 5].try_into().unwrap());
    assert_eq!(0x48b9e9 + 5 + i64::from(rel), 0x4588e0);
}
