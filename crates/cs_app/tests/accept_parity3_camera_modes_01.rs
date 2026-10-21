//! Acceptance for Rally #1823 `PARITY3-CAMERA-MODES.01`: the static trace of
//! the original's external camera, look, track-target and zoom code,
//! recorded as cited `[[camera_mode_fact]]` rows of
//! `data/owner-static/engine-facts.toml` (finding
//! `docs/findings/2026-10-21-parity3-camera-modes-01-measured.md`).
//!
//! The committed table is parsed by the production reader; the ignored test
//! re-reads every cited instruction and constant from the owner's decrypted
//! image so a row cannot drift from the bytes it cites.

use cs_app::engine_facts::engine_facts;

const TABLE: &str = include_str!("../../../data/owner-static/engine-facts.toml");
const IMAGE_BASE: usize = 0x40_0000;

/// One `[[camera_mode_fact]]` row.
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
        .unwrap_or_else(|| panic!("camera_mode_fact lacks {name}"))
}

fn rows() -> Vec<Row> {
    TABLE
        .split("[[camera_mode_fact]]")
        .skip(1)
        .map(|chunk| {
            // A row ends at the next table header or the end of the file.
            let chunk = chunk.split("\n[").next().unwrap_or(chunk);
            assert_eq!(field(chunk, "claim"), "OWNER-STATIC-2026-10-21");
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
        .unwrap_or_else(|| panic!("no camera_mode_fact named {name}"))
}

fn f32_of(row: &Row) -> f32 {
    f32::from_le_bytes(row.bytes[..4].try_into().expect("four bytes"))
}

#[test]
fn accept_parity3_camera_modes_01_the_committed_table_parses_with_the_camera_rows() {
    let facts = engine_facts().expect("the committed table parses");
    let rows = rows();
    assert!(rows.len() >= 100, "only {} rows", rows.len());
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
fn accept_parity3_camera_modes_01_the_commands_map_to_the_recorded_modes() {
    let rows = rows();
    // Commands 50..54 each push one camera mode for the setter.
    for (command, push) in [(50, 1u8), (51, 2), (52, 3), (53, 4), (54, 9)] {
        let handler = row(&rows, &format!("handler.cmd{command}.mode"));
        assert_eq!(handler.bytes, [0x6a, push], "command {command}");
        assert!(handler.fact.contains(&format!("camera mode {push}")));
        row(&rows, &format!("register.cmd{command}"));
    }
    // The look-mode commands.
    for name in [
        "register.cmd55",
        "register.cmd56",
        "register.cmd57",
        "look.snap_handler",
        "look.smooth_handler",
        "look.track_handler_and",
    ] {
        row(&rows, name);
    }
    // Look directions 58..66 are polled by the snap/smooth reader (62 only
    // by the track branch).
    for command in [58, 59, 60, 61, 63, 64, 65, 66] {
        row(&rows, &format!("look.poll_cmd{command}"));
    }
    row(&rows, "look.poll_cmd62_recentre");
    // Zoom: commands 67/68, the step and the limits.
    assert_eq!(row(&rows, "zoom.in_poll").bytes, [0x6a, 0x43]);
    assert_eq!(row(&rows, "zoom.out_poll").bytes, [0x6a, 0x44]);
}

#[test]
fn accept_parity3_camera_modes_01_the_recorded_constants_are_the_measured_ones() {
    let rows = rows();
    assert!((f32_of(row(&rows, "fov.default")).to_degrees() - 60.0).abs() < 1e-3);
    assert!((f32_of(row(&rows, "fov.second_eye")).to_degrees() - 80.0).abs() < 1e-3);
    assert_eq!(f32_of(row(&rows, "zoom.scale")), 10.0);
    assert_eq!(
        row(&rows, "zoom.smooth_rate").bytes[1..],
        1.5f32.to_le_bytes()
    );
    assert_eq!(
        row(&rows, "look.smooth_pitch_rate").bytes[1..],
        3.0f32.to_le_bytes()
    );
    assert_eq!(
        row(&rows, "look.smooth_yaw_rate").bytes[1..],
        5.0f32.to_le_bytes()
    );
    assert_eq!(
        row(&rows, "look.chase_call").bytes.len(),
        5,
        "a call instruction"
    );
    // The parameter record: the loader's keys at their offsets.
    let params: Vec<&Row> = rows
        .iter()
        .filter(|row| row.name.starts_with("param."))
        .collect();
    assert_eq!(params.len(), 32);
    for (key, offset) in [
        ("dist", "0x0"),
        ("dist_min", "0x10"),
        ("dist_max", "0x14"),
        ("back_dist_min", "0x18"),
        ("back_dist_max", "0x1c"),
        ("pos_catch_up", "0x20"),
        ("thirdp_height", "0x24"),
        ("thirdp_pitch", "0x28"),
        ("look_catch_up", "0x2c"),
    ] {
        let row = row(&rows, &format!("param.{key}"));
        assert!(
            row.fact.ends_with(&format!("record offset {offset}")),
            "{key}: {}",
            row.fact
        );
    }
}

#[test]
#[ignore = "requires CS_ENGINE_IMAGE"]
fn accept_parity3_camera_modes_01_cited_rows_match_the_engine_image() {
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
    // Strings the loader and the key rows cite.
    let cstr = |va: usize| {
        let start = va - IMAGE_BASE;
        let end = image[start..].iter().position(|&b| b == 0).unwrap() + start;
        String::from_utf8_lossy(&image[start..end]).into_owned()
    };
    assert_eq!(cstr(0x621848), "camparam.zrd");
    assert_eq!(cstr(0x621858), "default");
    for row in rows().iter().filter(|row| row.name.starts_with("param.")) {
        let string = u32::from_le_bytes(row.bytes[1..5].try_into().unwrap()) as usize;
        assert_eq!(
            format!("param.{}", cstr(string)),
            row.name,
            "the key string the row cites"
        );
    }
    // The airframe name table the per-airframe records are indexed by.
    let word = |va: usize| {
        u32::from_le_bytes(
            image[va - IMAGE_BASE..va - IMAGE_BASE + 4]
                .try_into()
                .unwrap(),
        ) as usize
    };
    let names: Vec<String> = (0..11).map(|i| cstr(word(0x620c70 + 28 * i))).collect();
    assert_eq!(
        names,
        [
            "Autogyro",
            "Hellhound",
            "Balmoral",
            "Bloodhawk",
            "Brigand",
            "Devastator",
            "Firebrand",
            "Fury",
            "Kestrel",
            "Peacemaker",
            "Warhawk"
        ]
    );
}
