//! M01-FOG-UPDATE-STATIC (#1847): the static trace of the engine's fog state
//! (finding `docs/findings/2026-10-23-m01-fog-update-static.md`).
//!
//! * the committed `[[fog_state]]` rows parse under the production loader and
//!   agree with the finding's numbers;
//! * with the owner's image named, every row is re-read from the image's bytes
//!   and the instruction bytes the finding's claims rest on are re-read too.

use cs_app::engine_facts::engine_facts;
use cs_content::coordinates::{load_engine_image, original_image_digest};

const TABLE_TEXT: &str = include_str!("../../../../data/owner-static/engine-facts.toml");

/// The number of `[[fog_state]]` rows the finding commits.
const ROW_COUNT: usize = 6;

struct Row {
    name: String,
    va: u64,
    value: f32,
}

fn rows() -> Vec<Row> {
    let mut rows = Vec::new();
    let mut current: Option<Vec<(String, String)>> = None;
    let finish = |entries: Option<Vec<(String, String)>>, rows: &mut Vec<Row>| {
        let Some(entries) = entries else { return };
        let get = |key: &str| -> String {
            entries
                .iter()
                .find(|(k, _)| k == key)
                .unwrap_or_else(|| panic!("a [[fog_state]] row lacks `{key}`"))
                .1
                .clone()
        };
        assert_eq!(get("provenance"), "\"static\"");
        assert_eq!(get("claim"), "\"OWNER-STATIC-2026-10-23\"");
        assert_eq!(get("kind"), "\"f32\"");
        let va = get("va");
        rows.push(Row {
            name: get("name").trim_matches('"').to_owned(),
            va: u64::from_str_radix(va.trim_start_matches("0x"), 16).expect("a hex va"),
            value: get("value").parse().expect("an f32 value"),
        });
    };
    for line in TABLE_TEXT.lines() {
        let line = line.trim();
        if line.starts_with('[') {
            finish(current.take(), &mut rows);
            if line == "[[fog_state]]" {
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

fn named(rows: &[Row], name: &str) -> f32 {
    rows.iter()
        .find(|row| row.name == name)
        .unwrap_or_else(|| panic!("row {name} is committed"))
        .value
}

#[test]
fn accept_m01_fog_update_static_rows_parse_and_agree_with_the_finding() {
    engine_facts().expect("the committed table parses with the fog rows in it");
    let rows = rows();
    assert_eq!(rows.len(), ROW_COUNT);
    assert_eq!(named(&rows, "fog_default_near"), 500.0);
    assert_eq!(named(&rows, "fog_default_far"), 700.0);
    assert_eq!(named(&rows, "fog_default_altitude_upper"), 300.0);
    assert_eq!(named(&rows, "fog_default_altitude_lower"), 200.0);
    // The cinematic exit undoes the entry exactly.
    assert_eq!(
        named(&rows, "cinematic_enter_far_clip_factor")
            * named(&rows, "cinematic_leave_far_clip_factor"),
        1.0
    );
}

#[test]
#[ignore = "requires CS_ENGINE_IMAGE"]
fn accept_m01_fog_update_static_rows_and_instructions_match_the_image() {
    let loaded = load_engine_image().expect("CS_ENGINE_IMAGE names the measured image");
    assert_eq!(loaded.digest, original_image_digest());
    let facts = engine_facts().expect("the committed table parses");
    let image = loaded.bytes.as_slice();
    let at = |va: u64| -> usize {
        usize::try_from(facts.image_offset(va).expect("va inside the image")).expect("an offset")
    };

    for row in rows() {
        let offset = at(row.va);
        let bytes: [u8; 4] = image[offset..offset + 4].try_into().expect("four bytes");
        assert_eq!(
            f32::from_le_bytes(bytes),
            row.value,
            "{} drifted at {:#x}",
            row.name,
            row.va
        );
    }

    let bytes = |va: u64, want: &[u8], what: &str| {
        let offset = at(va);
        assert_eq!(
            &image[offset..offset + want.len()],
            want,
            "{what} at {va:#x}"
        );
    };
    // The setters: range stores state +0x20/+0x24 and sets dirty bit 0x4;
    // altitude stores state +0x28 (third argument) and +0x2c (second) with
    // dirty bit 0x20.
    bytes(
        0x4da87f,
        &[
            0x89, 0x48, 0x20, 0x8b, 0x08, 0x83, 0xc9, 0x04, 0x89, 0x50, 0x24, 0x89, 0x08,
        ],
        "range setter 0x4da870 stores",
    );
    bytes(
        0x4da85f,
        &[
            0x89, 0x48, 0x28, 0x8b, 0x08, 0x83, 0xc9, 0x20, 0x89, 0x50, 0x2c, 0x89, 0x08,
        ],
        "altitude setter 0x4da850 stores",
    );
    // The flush 0x4da1b0: dirty bit 0x4 sends +0x20/+0x24 to 0x54e230 and
    // 0x54e280, dirty bit 0x20 sends +0x28/+0x2c to 0x54e2c0 and 0x54e300.
    bytes(
        0x4da23f,
        &[
            0xf6, 0x06, 0x04, 0x74, 0x1a, 0x8b, 0x4e, 0x20, 0x51, 0xbf, 0x01, 0x00, 0x00, 0x00,
            0xe8, 0xde, 0x3f, 0x07, 0x00, 0x8b, 0x56, 0x24, 0x52, 0xe8, 0x25, 0x40, 0x07, 0x00,
            0x83, 0xc4, 0x08, 0xf6, 0x06, 0x20, 0x74, 0x1a, 0x8b, 0x46, 0x28, 0x50, 0xbf, 0x01,
            0x00, 0x00, 0x00, 0xe8, 0x4f, 0x40, 0x07, 0x00, 0x8b, 0x4e, 0x2c, 0x51, 0xe8, 0x86,
            0x40, 0x07, 0x00, 0x83, 0xc4, 0x08, 0xf6,
        ],
        "flush dirty-bit dispatch",
    );
    // The upper altitude lands in 0xa06f60 (0x54e2c0 stores its argument),
    // and the per-vertex evaluator 0x578120 compares against 0xa06f60 and
    // 0xa06f64.
    bytes(
        0x54e2c0,
        &[
            0xd9, 0x44, 0x24, 0x04, 0x8b, 0x44, 0x24, 0x04, 0xd8, 0x25, 0x64, 0x6f, 0xa0, 0x00,
        ],
        "upper altitude store",
    );
    bytes(
        0x578199,
        &[0xd8, 0x1d, 0x60, 0x6f, 0xa0, 0x00],
        "evaluator: height minus radius against the upper altitude",
    );
    bytes(
        0x5781c9,
        &[0xd8, 0x1d, 0x64, 0x6f, 0xa0, 0x00],
        "evaluator: height plus radius against the lower altitude",
    );
    // The cinematic entry sets fog mode 0 and 0x654120 = 1 and multiplies
    // the far clip by 4.0; the exit sets fog mode 1 and clears 0x654120 after
    // multiplying by 0.25. Both write the fog mode of the object at 0x71c2ac.
    bytes(
        0x450676,
        &[0x8b, 0x15, 0xac, 0xc2, 0x71, 0x00],
        "entry: mode-setter object is [0x71c2ac]",
    );
    bytes(
        0x45067c,
        &[0x6a, 0x00, 0x52, 0xe8, 0x7c, 0xa1, 0x08],
        "entry: fog mode 0",
    );
    bytes(
        0x450562,
        &[0xc7, 0x05, 0x20, 0x41, 0x65, 0x00, 0x01, 0x00, 0x00, 0x00],
        "entry: 0x654120 = 1",
    );
    bytes(
        0x4506a6,
        &[0xd8, 0x0d, 0x14, 0x35, 0x60, 0x00],
        "entry: far clip times 4.0",
    );
    bytes(
        0x450ee1,
        &[0x6a, 0x01, 0x52, 0xe8, 0x17, 0x99, 0x08],
        "exit: fog mode 1",
    );
    bytes(
        0x450f0b,
        &[0xd8, 0x0d, 0xf4, 0x34, 0x60, 0x00],
        "exit: far clip times 0.25",
    );
    bytes(
        0x450f9b,
        &[0xc7, 0x05, 0x20, 0x41, 0x65, 0x00, 0x00, 0x00, 0x00, 0x00],
        "exit: 0x654120 = 0",
    );
    bytes(
        0x450edb,
        &[0x8b, 0x15, 0xac, 0xc2, 0x71, 0x00],
        "exit: mode-setter object is [0x71c2ac]",
    );
    // The WorldSetFogDensity handler reaches the density setter.
    bytes(
        0x5bc78c,
        &[0xe8, 0xff, 0xe0, 0xf1, 0xff],
        "WorldSetFogDensity handler call",
    );
    // The zone apply runs from the camera-zone change (0x42f283) and writes
    // the three fog setters with the zone record.
    bytes(
        0x42f26e,
        &[
            0x3b, 0xc8, 0x5f, 0x5e, 0x74, 0x18, 0x8a, 0x0d, 0x78, 0x13, 0x62, 0x00, 0x84, 0xc9,
            0xa3, 0x6c, 0xef, 0x64, 0x00, 0x74, 0x09, 0x50,
        ],
        "camera-zone change guard",
    );
    bytes(
        0x472ee2,
        &[0xa1, 0xb8, 0xc0, 0x71, 0x00],
        "zone apply: world object",
    );
    // The cloud band's in-layer effect is a screen colour: the per-frame
    // render pass calls the tint query 0x42ee40 and, when its intensity is
    // above zero, the screen fill 0x5ca1f0.
    bytes(0x42f49e, &[0xe8, 0x9d, 0xf9, 0xff, 0xff], "tint query call");
    bytes(
        0x42f4c4,
        &[0xd9, 0x44, 0x24, 0x04, 0xd8, 0x1d, 0xc8, 0x32, 0x60, 0x00],
        "tint intensity against zero",
    );
    bytes(
        0x42f553,
        &[0xe8, 0x98, 0xac, 0x19, 0x00],
        "screen fill call",
    );
    // The precipitation controller 0x42b7c0 compares camera Y with BOTTOM;
    // a second per-frame site calls it with the same camera object.
    bytes(
        0x42b80d,
        &[0xd8, 0x1d, 0xd8, 0xdc, 0x71, 0x00],
        "controller: Y against BOTTOM",
    );
    bytes(
        0x489e4e,
        &[0xe8, 0x6d, 0x19, 0xfa, 0xff],
        "second controller call site",
    );
}
