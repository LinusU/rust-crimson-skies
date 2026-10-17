//! M01-WINGMAN-FORMATION-A (#1563): the committed `[[ai_follow]]` rows of
//! `data/owner-static/engine-facts.toml` — the follow/formation constants of
//! the mode-4 AI routine `0x41e760` and its control writer `0x41b560`
//! (finding `docs/findings/2026-10-17-m01-wingman-formation-control-law.md`).
//!
//! The typed loader does not consume these rows yet (no runtime changed), so
//! this test reads the committed text itself and pins two things: the table
//! still parses under the production loader with the rows in it, and — with
//! the owner's image named — every row's value is read back out of the
//! image's own bytes, so the table cannot silently drift.

use cs_app::engine_facts::engine_facts;
use cs_content::coordinates::{load_engine_image, original_image_digest};

const TABLE_TEXT: &str = include_str!("../../../../data/owner-static/engine-facts.toml");

/// The number of rows the finding commits.
const ROW_COUNT: usize = 27;

struct Row {
    name: String,
    va: u64,
    kind: String,
    value: String,
}

fn ai_follow_rows() -> Vec<Row> {
    let mut rows = Vec::new();
    let mut current: Option<Vec<(String, String)>> = None;
    let finish = |entries: Option<Vec<(String, String)>>, rows: &mut Vec<Row>| {
        let Some(entries) = entries else { return };
        let get = |key: &str| -> String {
            entries
                .iter()
                .find(|(k, _)| k == key)
                .unwrap_or_else(|| panic!("an [[ai_follow]] row lacks `{key}`"))
                .1
                .clone()
        };
        assert_eq!(get("claim"), "\"OWNER-STATIC-2026-10-17\"");
        assert_eq!(get("provenance"), "\"static\"");
        let va = get("va");
        rows.push(Row {
            name: get("name").trim_matches('"').to_owned(),
            va: u64::from_str_radix(va.trim_start_matches("0x"), 16).expect("a hex va"),
            kind: get("kind").trim_matches('"').to_owned(),
            value: get("value"),
        });
    };
    for line in TABLE_TEXT.lines() {
        let line = line.trim();
        if line.starts_with('[') {
            finish(current.take(), &mut rows);
            if line == "[[ai_follow]]" {
                current = Some(Vec::new());
            }
        } else if let Some(entries) = current.as_mut()
            && let Some((key, value)) = line.split_once(" = ")
        {
            entries.push((key.to_owned(), value.to_owned()));
        }
    }
    finish(current.take(), &mut rows);
    rows
}

fn value_f32(row: &Row) -> f32 {
    row.value.parse().expect("an f32 value")
}

#[test]
fn accept_m01_wingman_formation_a_rows_parse_and_agree_with_the_finding() {
    engine_facts().expect("the committed table parses with the ai_follow rows in it");
    let rows = ai_follow_rows();
    assert_eq!(
        rows.len(),
        ROW_COUNT,
        "the finding commits {ROW_COUNT} rows"
    );
    let mut names: Vec<&str> = rows.iter().map(|row| row.name.as_str()).collect();
    names.sort_unstable();
    names.dedup();
    assert_eq!(names.len(), ROW_COUNT, "row names are unique");

    let by_name = |name: &str| -> &Row {
        rows.iter()
            .find(|row| row.name == name)
            .unwrap_or_else(|| panic!("row {name} is committed"))
    };
    // The finding's headline numbers, spelled from the rows.
    assert_eq!(value_f32(by_name("follow_speed_band")), 26.8224);
    assert_eq!(value_f32(by_name("follow_throttle_min")), 0.4);
    assert_eq!(value_f32(by_name("follow_throttle_max")), 1.5);
    assert_eq!(
        value_f32(by_name("follow_rubber_band_dist2")).sqrt(),
        2000.0
    );
    assert_eq!(value_f32(by_name("follow_engage_dist2")).sqrt(), 700.0);
    assert_eq!(by_name("wingman_mode_value").value, "4");
    let station = |axis: &str| value_f32(by_name(&format!("station_offset_other_leader_{axis}")));
    assert_eq!(
        (station("x"), station("y"), station("z")),
        (8.0, -2.0, -8.0)
    );
}

#[test]
#[ignore = "requires CS_ENGINE_IMAGE"]
fn accept_m01_wingman_formation_a_rows_match_the_image() {
    let loaded = load_engine_image().expect("CS_ENGINE_IMAGE names the measured image");
    assert_eq!(loaded.digest, original_image_digest());
    let facts = engine_facts().expect("the committed table parses");
    let image = loaded.bytes.as_slice();
    let rows = ai_follow_rows();
    assert_eq!(rows.len(), ROW_COUNT);
    for row in &rows {
        let offset = usize::try_from(facts.image_offset(row.va).expect("va inside the image"))
            .expect("an offset");
        let bytes: [u8; 4] = image[offset..offset + 4].try_into().expect("four bytes");
        match row.kind.as_str() {
            "f32" => assert_eq!(
                f32::from_le_bytes(bytes).to_bits(),
                value_f32(row).to_bits(),
                "{} drifted at {:#x}",
                row.name,
                row.va
            ),
            "u32" => assert_eq!(
                u32::from_le_bytes(bytes).to_string(),
                row.value,
                "{} drifted at {:#x}",
                row.name,
                row.va
            ),
            other => panic!("{}: unknown kind {other}", row.name),
        }
    }
}
