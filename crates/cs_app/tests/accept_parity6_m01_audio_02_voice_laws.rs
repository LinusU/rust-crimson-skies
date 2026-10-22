//! Acceptance for Rally #1862 `PARITY6-M01-AUDIO-TIMELINE.02`: the zSound
//! volume, pan and rate laws and the aircraft engine/prop voice laws, recorded
//! as cited `[[audio_fact]]` rows of `data/owner-static/engine-facts.toml`
//! (finding `docs/findings/2026-10-22-m01-audio-timeline-voice-laws.md`).
//!
//! The committed table is parsed by the production reader; the ignored test
//! re-reads every cited byte and every cited call target from the owner's
//! decrypted image so a row cannot drift from the bytes it cites.
//! Task test prefix: `accept_parity6_m01_audio_02_`.

use cs_app::engine_facts::engine_facts;

const TABLE: &str = include_str!("../../../data/owner-static/engine-facts.toml");
const IMAGE_BASE: usize = 0x40_0000;

/// One `[[audio_fact]]` row.
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
        .unwrap_or_else(|| panic!("audio_fact lacks {name}"))
}

fn rows() -> Vec<Row> {
    TABLE
        .split("[[audio_fact]]")
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
        .unwrap_or_else(|| panic!("no audio_fact named {name}"))
}

fn f32_of(row: &Row) -> f32 {
    assert_eq!(row.bytes.len(), 4, "{} is not a single f32", row.name);
    f32::from_le_bytes(row.bytes.as_slice().try_into().unwrap())
}

fn contains(haystack: &[u8], needle: &[u8]) -> bool {
    haystack
        .windows(needle.len())
        .any(|window| window == needle)
}

#[test]
fn accept_parity6_m01_audio_02_the_committed_table_parses_with_the_audio_rows() {
    let facts = engine_facts().expect("the committed table parses");
    let rows = rows();
    assert_eq!(rows.len(), 40);
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
fn accept_parity6_m01_audio_02_the_rate_pan_and_volume_constants_are_the_cited_values() {
    let rows = rows();
    // Rate setter: 4000 Hz floor, 51,200 Hz span, clamp 4000..55200.
    assert_eq!(f32_of(row(&rows, "zsnd.pitch_setter.constant")), -51200.0);
    let law = &row(&rows, "zsnd.pitch_setter.law").bytes;
    assert!(contains(law, &4000u32.to_le_bytes()));
    assert!(contains(law, &55200u32.to_le_bytes()));
    // Pan setter: -4000..+4000 in 1/100 dB from a 0..1 argument.
    assert_eq!(f32_of(row(&rows, "zsnd.pan_setter.constant")), -8000.0);
    let law = &row(&rows, "zsnd.pan_setter.law").bytes;
    assert!(contains(law, &(-4000i32).to_le_bytes()));
    assert!(contains(law, &4000u32.to_le_bytes()));
    // Gain to 1/100 dB: 1000 per octave, floor at 1/1024 -> -10000.
    assert_eq!(f32_of(row(&rows, "zsnd.gain_slope_constant")), 1000.0);
    assert_eq!(f32_of(row(&rows, "zsnd.gain_floor_constant")), 1.0 / 1024.0);
    // 3D pan reaches 1600 (16 dB) on the listener axis.
    assert_eq!(f32_of(row(&rows, "zsnd3d.pan_constant")), 1600.0);
    // The 3D update silences with -10000 and clamps the Doppler rate to
    // 100..100000 Hz.
    assert!(contains(
        &row(&rows, "zsnd3d.doppler_apply").bytes,
        &100_000u32.to_le_bytes()
    ));
    assert!(contains(
        &row(&rows, "zsnd3d.doppler_apply").bytes,
        &100u32.to_le_bytes()
    ));
    // A non-player aircraft keeps its engine voices inside 2000 m (squared).
    assert_eq!(f32_of(row(&rows, "engine.audible_limit")), 2000.0 * 2000.0);
    assert!(row(&rows, "zsnd3d.doppler_factor").fact.contains("345"));
}

#[test]
fn accept_parity6_m01_audio_02_the_engine_config_defaults_and_limits_are_recorded() {
    let rows = rows();
    // The engine_sound and prop_sound defaults are 0.1 and 0.9 floats.
    for name in ["config.engine_sound_defaults", "config.prop_sound_defaults"] {
        let bytes = &row(&rows, name).bytes;
        assert!(contains(bytes, &0.1f32.to_le_bytes()), "{name}");
        assert!(contains(bytes, &0.9f32.to_le_bytes()), "{name}");
    }
    // The rattle defaults: volume_range (0.25, 1.0), speed_range (1.0, 2.0).
    let rattle = &row(&rows, "config.rattle_defaults").bytes;
    for value in [0.25f32, 1.0, 2.0] {
        assert!(contains(rattle, &value.to_le_bytes()));
    }
    // What the image does not do is part of the record: nothing re-reads the
    // rattle volume range, and the burst and cinematic starters stay unknown
    // in the finding.
    assert!(
        row(&rows, "config.rattle_defaults")
            .fact
            .contains("no code reads")
    );
    assert!(row(&rows, "rattle.consumer").fact.contains("not decoded"));
    assert!(row(&rows, "engine.gain_chain").fact.contains("not decoded"));
    // The sound-node update has no distance test of its own.
    assert!(
        row(&rows, "soundnode.update")
            .fact
            .contains("no distance test")
    );
}

#[test]
#[ignore = "requires CS_ENGINE_IMAGE"]
fn accept_parity6_m01_audio_02_cited_rows_match_the_engine_image() {
    let path = std::env::var("CS_ENGINE_IMAGE").expect("CS_ENGINE_IMAGE must name the image");
    let image = std::fs::read(path).expect("read the engine image");
    for row in rows() {
        let offset = row.va - IMAGE_BASE;
        // `.text` and `.rdata` are mapped at the same offset from the base
        // for these addresses (the committed table's convention).
        assert_eq!(
            &image[offset..offset + row.bytes.len()],
            row.bytes.as_slice(),
            "{} at {:#x}",
            row.name,
            row.va
        );
    }
    // The cited call sites land on the functions the facts name.
    for (site, target) in [
        (0x597b30usize, 0x597c20usize),
        (0x4b1e03, 0x597740),
        (0x4b1e21, 0x5978f0),
        (0x4b1f25, 0x597740),
        (0x4b1f43, 0x5978f0),
        (0x4e0df2, 0x593b80),
        (0x4e0e3b, 0x593590),
        (0x4b14a6, 0x593b80),
    ] {
        let at = site - IMAGE_BASE;
        assert_eq!(image[at], 0xe8, "call opcode at {site:#x}");
        let rel = i32::from_le_bytes(image[at + 1..at + 5].try_into().unwrap());
        assert_eq!(
            site as i64 + 5 + i64::from(rel),
            target as i64,
            "call at {site:#x}"
        );
    }
    // The DirectSound vtable calls of the 3D update: GetFrequency (0x20),
    // SetFrequency (0x44), SetPan (0x40), SetVolume (0x3c); each is
    // `call dword ptr [reg + slot]` (ff /2 disp8).
    for (site, modrm, slot) in [
        (0x597f57usize, 0x52u8, 0x20u8),
        (0x597f9d, 0x52, 0x44),
        (0x597fcd, 0x51, 0x40),
        (0x597fd7, 0x50, 0x3c),
    ] {
        let at = site - IMAGE_BASE;
        assert_eq!(image[at..at + 3], [0xff, modrm, slot], "{site:#x}");
    }
}
