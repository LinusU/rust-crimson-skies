//! M01-WINGMAN-PILOT-GAINS (#2266): the follower's pilot input scales and
//! limits and the rudder tolerance, read from the owner image and from the
//! retail archives.
//!
//! Task test prefix: `accept_m01_wingman_gains_`. The writers are decoded in
//! `docs/findings/2026-11-02-m01-wingman-gains.md`:
//!
//! * the definition defaults: `def+0x260` = `0.2` (`0x478dd6`),
//!   `def+0xa8/ac/b0/b4/b8/bc` = `2.0`/`0.5`/`0.15`/`100`/`100`/`0.25`
//!   (`0x478b10`..`0x478b44`), `def+0x264/0x268/0x26c` = `3.5`
//!   (`0x478de0`..`0x478dec`) and `def+0x270/0x274/0x278` = `1.0`
//!   (`0x478df2`..`0x478dfe`);
//! * the definition parser's key→slot writes: `rudder_tol` at `def+0x260`
//!   (`0x47b37e`), `mode_alt` at `def+0xa8` (`0x47b0a6`), `collision_d` at
//!   `def+0xac`/`+0xb0` (`0x47b0c4`/`0x47b0d0`), and the six `ai_input_*`
//!   keys at `def+0x264..+0x278` (`0x47b4bd`..`0x47b553`);
//! * the spawn copy `0x47d1e9`..`0x47d334` that fills `vehicle+0x8c4..+0x8d8`
//!   from `arg1+0xa8..+0xbc` (the definition, `0x47c2b4`'s kind test) and
//!   falls back to `def+0x264..+0x278` on the `-1.0` sentinel, plus the
//!   `+0x8dc..+0x8f0` `ai_emerg_input_*` set that step 7's `bl` argument
//!   selects instead; the finding records which reading the retail data
//!   supports and what stays unmeasured;
//! * step 7 of the stick law (`0x41be53`..`0x41bf8e`) that scales each word
//!   and clamps it to the limit.

use std::path::Path;

use cs_app::mission_ai::{
    M01_WINGMAN_PILOT_GAINS, PILOT_GAINS_AI_INPUT_KEYS, PILOT_GAINS_SPAWN_WORDS, PilotInputGains,
    RUDDER_TOL, StickWords,
};
use cs_app::mission_start::read_aircraft_records;
use cs_content::stunts::{ZrdValue, decode_zrd};
use cs_types::asset_id::SourceSpan;

use crate::campaign::vs_m01_rt_window::game_dir;

/// A test-local assertion helper: the four bytes at `va` in the owner image.
fn f32_at(bytes: &[u8], va: u64) -> f32 {
    let at = usize::try_from(va - 0x400000).expect("image offset");
    f32::from_le_bytes(bytes[at..at + 4].try_into().expect("four bytes"))
}

/// The raw bytes of `count` instructions starting at `va`.
fn bytes_at(bytes: &[u8], va: u64, count: usize) -> &[u8] {
    let at = usize::try_from(va - 0x400000).expect("image offset");
    &bytes[at..at + count]
}

/// The null-terminated text at `va` in the owner image.
fn text_at(bytes: &[u8], va: u64) -> String {
    let at = usize::try_from(va - 0x400000).expect("image offset");
    let end = bytes[at..].iter().position(|byte| *byte == 0).expect("NUL") + at;
    String::from_utf8(bytes[at..end].to_vec()).expect("ASCII key")
}

/// The measured pilot words: the spawn copy's first source (which is what
/// M01's `wingman` definition holds) and the `ai_input_*` fallback it never
/// reaches, plus the two numbers the ported law's step 7 applies to each
/// word — its scale first, then its limit.
#[test]
fn accept_m01_wingman_gains_scale_then_limit_shape_each_word() {
    assert_eq!(RUDDER_TOL, 0.2);
    assert_eq!(M01_WINGMAN_PILOT_GAINS.scale, [0.5, 2.0, 0.15]);
    assert_eq!(M01_WINGMAN_PILOT_GAINS.limit, [100.0, 100.0, 0.25]);
    assert_eq!(PILOT_GAINS_AI_INPUT_KEYS.scale, [3.5, 3.5, 3.5]);
    assert_eq!(PILOT_GAINS_AI_INPUT_KEYS.limit, [1.0, 1.0, 1.0]);
    assert_eq!(M01_WINGMAN_PILOT_GAINS, PILOT_GAINS_SPAWN_WORDS);

    // The M01 words: roll is halved, pitch is doubled, yaw is scaled to
    // 0.15 and capped at 0.25; the roll and pitch limits are the flight
    // input's own range.
    let small = StickWords {
        a100: 0.1,
        a108: 0.1,
        a10c: 0.1,
        recover_throttle: false,
    };
    let scaled = small.scaled(M01_WINGMAN_PILOT_GAINS);
    assert!((scaled.a100 - 0.05).abs() < 1e-6, "{scaled:?}");
    assert!((scaled.a108 - 0.2).abs() < 1e-6, "{scaled:?}");
    assert!((scaled.a10c - 0.015).abs() < 1e-6, "{scaled:?}");

    let big = StickWords {
        a100: 0.5,
        a108: -0.5,
        a10c: 1.0,
        recover_throttle: false,
    };
    let scaled = big.scaled(M01_WINGMAN_PILOT_GAINS);
    assert_eq!(scaled.a100, 0.25, "0.5 * 0.5");
    assert_eq!(scaled.a108, -1.0, "-0.5 * 2.0");
    assert_eq!(scaled.a10c, 0.15, "1.0 * 0.15");
    let pinned = StickWords { a10c: 2.0, ..big }.scaled(M01_WINGMAN_PILOT_GAINS);
    assert_eq!(pinned.a10c, 0.25, "the yaw limit binds at 0.25");

    // The `ai_input_*` fallback: 3.5 amplification then a +/-1 limit.
    let scaled = small.scaled(PILOT_GAINS_AI_INPUT_KEYS);
    assert!((scaled.a100 - 0.35).abs() < 1e-6, "{scaled:?}");
    assert!((scaled.a108 - 0.35).abs() < 1e-6, "{scaled:?}");
    assert!((scaled.a10c - 0.35).abs() < 1e-6, "{scaled:?}");
    let scaled = big.scaled(PILOT_GAINS_AI_INPUT_KEYS);
    assert_eq!(scaled.a100, 1.0, "1.75 clamps to 1.0");
    assert_eq!(scaled.a108, -1.0, "-1.75 clamps to -1.0");

    // The flight input negates the roll word and passes pitch and yaw.
    let input = small.flight_input(0.7);
    assert!(input.roll < 0.0, "a100 drives a left roll word");
    assert!(input.pitch > 0.0);
    assert!(input.yaw > 0.0);
    assert_eq!(input.throttle, 0.7);
}

/// The measured constants are the ones the crate flies, so a re-tuned
/// constant cannot quietly pass the flight pins.
#[test]
fn accept_m01_wingman_gains_constants_are_the_measured_ones() {
    assert_eq!(
        PILOT_GAINS_SPAWN_WORDS,
        PilotInputGains {
            scale: [0.5, 2.0, 0.15],
            limit: [100.0, 100.0, 0.25],
        }
    );
    assert_eq!(
        PILOT_GAINS_AI_INPUT_KEYS,
        PilotInputGains {
            scale: [3.5, 3.5, 3.5],
            limit: [1.0, 1.0, 1.0],
        }
    );
    assert_eq!(M01_WINGMAN_PILOT_GAINS, PILOT_GAINS_SPAWN_WORDS);
}

/// **The byte windows of every writer.** The definition constructor's
/// defaults, the parser's key→slot writes with their key texts, the spawn
/// copy, the record document's sentinel/default and the law's step 7.
#[test]
#[ignore = "requires CS_ENGINE_IMAGE"]
fn accept_m01_wingman_gains_image_pins_the_writer_windows() {
    let image = cs_content::coordinates::load_engine_image()
        .expect("CS_ENGINE_IMAGE names the measured image");
    let bytes = &image.bytes;

    // 1. The definition constructor's defaults (`0x478a00`).
    assert_eq!(
        bytes_at(bytes, 0x478dcb, 5),
        &[0xb9, 0x00, 0x00, 0x60, 0x40],
        "mov ecx, 0x40600000 (3.5) feeds the three scales"
    );
    assert_eq!(f32_at(bytes, 0x478ddc), RUDDER_TOL, "def+0x260 = 0.2");
    for (site, offset) in [
        (0x478de0_u64, 0x264_u64),
        (0x478de6, 0x268),
        (0x478dec, 0x26c),
    ] {
        let window = bytes_at(bytes, site, 6);
        assert_eq!(window[0], 0x89, "a dword store at {site:#x}");
        assert_eq!(
            u32::from_le_bytes(window[2..6].try_into().expect("disp")),
            u32::try_from(offset).expect("offset"),
            "the scale store at {site:#x} names def+{offset:#x}"
        );
    }
    for (site, offset) in [
        (0x478df2_u64, 0x270_u64),
        (0x478df8, 0x274),
        (0x478dfe, 0x278),
    ] {
        let window = bytes_at(bytes, site, 6);
        assert_eq!(window[0], 0x89, "a dword store at {site:#x}");
        assert_eq!(
            u32::from_le_bytes(window[2..6].try_into().expect("disp")),
            u32::try_from(offset).expect("offset"),
            "the limit store at {site:#x} names def+{offset:#x}"
        );
    }
    assert_eq!(
        bytes_at(bytes, 0x478af5, 5),
        &[0xb8, 0x00, 0x00, 0x80, 0x3f],
        "mov eax, 0x3f800000 (1.0) feeds the three limits"
    );

    // 2. The definition parser's key→slot writes and the key texts.
    for (key_va, store_va, offset) in [
        (0x6281a4_u64, 0x47b37e_u64, 0x260_u64),
        (0x628220, 0x47b4bd, 0x268),
        (0x628238, 0x47b4db, 0x264),
        (0x62824c, 0x47b4f9, 0x26c),
        (0x628260, 0x47b517, 0x274),
        (0x628278, 0x47b535, 0x270),
        (0x62828c, 0x47b553, 0x278),
    ] {
        let window = bytes_at(bytes, store_va, 6);
        assert_eq!(
            u32::from_le_bytes(window[2..6].try_into().expect("disp")),
            u32::try_from(offset).expect("offset"),
            "the parser store at {store_va:#x} names def+{offset:#x} \
             for the key at {key_va:#x} ({})",
            text_at(bytes, key_va)
        );
    }
    assert_eq!(text_at(bytes, 0x6281a4), "rudder_tol");
    assert_eq!(text_at(bytes, 0x628220), "ai_input_scale_pitch");
    assert_eq!(text_at(bytes, 0x628238), "ai_input_scale_roll");
    assert_eq!(text_at(bytes, 0x62824c), "ai_input_scale_yaw");
    assert_eq!(text_at(bytes, 0x628260), "ai_input_limit_pitch");
    assert_eq!(text_at(bytes, 0x628278), "ai_input_limit_roll");
    assert_eq!(text_at(bytes, 0x62828c), "ai_input_limit_yaw");

    // 3. The record document's sentinel defaults and the positional fields
    // that overwrite them (`aiv.zrd` fields 44..49).
    for (site, offset) in [
        (0x437420_u64, 0xa8_u64),
        (0x437426, 0xac),
        (0x43742c, 0xb0),
        (0x437432, 0xb4),
        (0x437438, 0xb8),
        (0x43743e, 0xbc),
    ] {
        let window = bytes_at(bytes, site, 6);
        assert_eq!(
            u32::from_le_bytes(window[2..6].try_into().expect("disp")),
            u32::try_from(offset).expect("offset"),
            "the record constructor's -1.0 default at {site:#x} names doc+{offset:#x}"
        );
    }
    for (site, offset) in [(0x437ca9_u64, 0xa8_u64), (0x437cf4, 0xbc)] {
        let window = bytes_at(bytes, site, 6);
        assert_eq!(
            u32::from_le_bytes(window[2..6].try_into().expect("disp")),
            u32::try_from(offset).expect("offset"),
            "the field parse at {site:#x} names doc+{offset:#x}"
        );
    }

    // 4. The spawn copy: the override read and the six stores.
    let read = bytes_at(bytes, 0x47d1e9, 6);
    assert_eq!(
        read,
        &[0x8b, 0x83, 0xa8, 0x00, 0x00, 0x00],
        "mov eax, [ebx+0xa8]"
    );
    for (site, offset) in [
        (0x47d1fb_u64, 0x8c8_u64),
        (0x47d222, 0x8c4),
        (0x47d249, 0x8cc),
        (0x47d270, 0x8d4),
        (0x47d2b9, 0x8d0),
        (0x47d334, 0x8d8),
    ] {
        let window = bytes_at(bytes, site, 6);
        assert_eq!(
            u32::from_le_bytes(window[2..6].try_into().expect("disp")),
            u32::try_from(offset).expect("offset"),
            "the spawn store at {site:#x} names vehicle+{offset:#x}"
        );
    }

    // 4b. The alternate `ai_emerg_input_*` set the same copy fills and step 7
    // selects when its `bl` argument is non-zero.
    for (site, offset) in [
        (0x47d347_u64, 0x8e0_u64),
        (0x47d36e, 0x8dc),
        (0x47d395, 0x8e4),
        (0x47d3bc, 0x8ec),
        (0x47d3e3, 0x8e8),
        (0x47d40a, 0x8f0),
    ] {
        let window = bytes_at(bytes, site, 6);
        assert_eq!(
            u32::from_le_bytes(window[2..6].try_into().expect("disp")),
            u32::try_from(offset).expect("offset"),
            "the emergency spawn store at {site:#x} names vehicle+{offset:#x}"
        );
    }
    assert_eq!(text_at(bytes, 0x6282a0), "ai_emerg_input_scale_pitch");

    // 5. Step 7 of the stick law: three scales then three limits.
    for (site, offset) in [
        (0x41be53_u64, 0x8c4_u64),
        (0x41be80, 0x8c8),
        (0x41bead, 0x8cc),
        (0x41bedd, 0x8d0),
        (0x41bf30, 0x8d4),
        (0x41bf83, 0x8d8),
    ] {
        let window = bytes_at(bytes, site, 6);
        assert_eq!(window[0], 0xd9, "an f32 load at {site:#x}: {window:?}");
        assert_eq!(
            u32::from_le_bytes(window[2..6].try_into().expect("disp")),
            u32::try_from(offset).expect("offset"),
            "the law's load at {site:#x} names vehicle+{offset:#x}"
        );
    }
    // The `bl` argument picks the emergency set when non-zero.
    assert_eq!(
        bytes_at(bytes, 0x41be47, 4),
        &[0x84, 0xdb, 0x74, 0x08],
        "test bl, bl / je over the emergency scale load"
    );
    for (site, offset) in [(0x41be4b_u64, 0x8dc_u64), (0x41bed5, 0x8e8)] {
        let window = bytes_at(bytes, site, 6);
        assert_eq!(window[0], 0xd9, "an f32 load at {site:#x}: {window:?}");
        assert_eq!(
            u32::from_le_bytes(window[2..6].try_into().expect("disp")),
            u32::try_from(offset).expect("offset"),
            "the emergency load at {site:#x} names vehicle+{offset:#x}"
        );
    }
    // The `+0.5` bonus global `0x64ee4c` is read between the scale and the
    // limit (`mov al, byte ptr [0x64ee4c]` at `0x41be59`), which this crate
    // flies clear.
    assert_eq!(
        bytes_at(bytes, 0x41be59, 5),
        &[0xa0, 0x4c, 0xee, 0x64, 0x00],
        "the gain-bonus byte gate"
    );
}

/// A test-local walk to one member's decoded document, the same production
/// path `m01_aiv_layout` uses.
fn member_document(root: &Path, container_key: &str, member: &str) -> (ZrdValue, SourceSpan) {
    let found = cs_assets::install::discover(root).expect("the installation discovers");
    let record = found
        .manifest
        .files
        .iter()
        .find(|record| record.relative_spelling.logical_key() == container_key.to_ascii_lowercase())
        .unwrap_or_else(|| panic!("the installation has {container_key}"));
    let spelling = record.relative_spelling.as_str().to_owned();
    let path = cs_types::install::RelativePath::new(&spelling.to_lowercase()).expect("a path");
    let bytes = std::fs::read(found.manifest.host_root.join(&spelling)).expect("readable");
    let discovery = cs_formats::script_raw::discover_container(container_key, &path, &bytes);
    let program = discovery
        .programs()
        .iter()
        .find(|program| {
            program
                .locator()
                .member()
                .is_some_and(|name| name.eq_ignore_ascii_case(member))
        })
        .unwrap_or_else(|| panic!("{container_key} declares {member}"));
    let document =
        decode_zrd(program.bytes()).unwrap_or_else(|error| panic!("{member} decodes: {error:?}"));
    let span = program.locator().span();
    let source = SourceSpan::new(
        cs_assets::install::fingerprint(&found.manifest),
        &spelling,
        Some(member),
        span.offset,
        span.len,
        None,
    )
    .expect("a member span");
    (document, source)
}

/// The `(name, record)` pairs of the retail `vehicle.zrd` document.
fn vehicle_records(document: &ZrdValue) -> Vec<(&str, &ZrdValue)> {
    let top = document.as_list().expect("the vehicle document is a list");
    let records = top
        .first()
        .and_then(ZrdValue::as_list)
        .expect("its only child");
    let mut pairs = Vec::new();
    let mut index = 0;
    while index + 1 < records.len() {
        if let Some(name) = records[index].as_text() {
            pairs.push((name, &records[index + 1]));
            index += 2;
        } else {
            index += 1;
        }
    }
    pairs
}

/// The value node of `key` in a flat record list.
fn record_field<'a>(record: &'a ZrdValue, key: &str) -> Option<&'a ZrdValue> {
    let fields = record.as_list()?;
    let mut index = 0;
    while index + 1 < fields.len() {
        if fields[index].as_text() == Some(key) {
            return fields.get(index + 1);
        }
        index += 2;
    }
    None
}

const AI_INPUT_KEYS: [&str; 7] = [
    "rudder_tol",
    "ai_input_scale_pitch",
    "ai_input_scale_roll",
    "ai_input_scale_yaw",
    "ai_input_limit_pitch",
    "ai_input_limit_roll",
    "ai_input_limit_yaw",
];

/// **The values M01's wingman actually gets.** The retail `wingman` record
/// declares none of the seven keys, so the definition keeps the constructor's
/// words. M01's `wingman_1` document carries `-1.0` in its own six optional
/// slots (fields 44..49); the spawn copy reads the definition, not those
/// slots (finding §3), so they pin the record's shape rather than override
/// the words.
#[test]
#[ignore = "requires CS_GAME_DIR"]
fn accept_m01_wingman_gains_retail_pins_m01_wingman_values() {
    let root = game_dir();

    let (vehicles, _span) = member_document(&root, "zbd/zrdr.zbd", "vehicle.zrd");
    let records = vehicle_records(&vehicles);
    let wingman = records
        .iter()
        .find(|(name, _)| name.eq_ignore_ascii_case("wingman"))
        .map(|(_, record)| *record)
        .expect("the retail vehicle registry declares `wingman`");
    for key in AI_INPUT_KEYS {
        assert!(
            record_field(wingman, key).is_none(),
            "the `wingman` record declares {key}: {:?}",
            record_field(wingman, key)
        );
    }
    // With neither key the definition keeps the constructor's words, which
    // are the ones M01's wingman flies.
    assert_eq!(M01_WINGMAN_PILOT_GAINS, PILOT_GAINS_SPAWN_WORDS);
    assert_eq!(M01_WINGMAN_PILOT_GAINS.scale, [0.5, 2.0, 0.15]);
    assert_eq!(M01_WINGMAN_PILOT_GAINS.limit, [100.0, 100.0, 0.25]);
    assert_eq!(RUDDER_TOL, 0.2);

    // M01's own records: `wingman_1` carries the -1.0 sentinel in its own
    // optional slots. The spawn copy reads the definition's words, not these
    // (finding §3); the fields are pinned so the instance's shape stays the
    // one the finding measured.
    let (aiv, source) = member_document(&root, "zbd/c3/m01/zrdr.zbd", "aiv.zrd");
    let aircraft = read_aircraft_records("zbd/c3/m01", &aiv, &source).expect("read");
    let betty = aircraft
        .iter()
        .find(|record| record.name == "wingman_1")
        .expect("M01's aiv.zrd declares wingman_1");
    assert_eq!(betty.field_count, 66, "the M01 record shape");

    let root_list = aiv.as_list().expect("the aiv document is a list");
    let record = root_list
        .iter()
        .filter_map(ZrdValue::as_list)
        .find(|entry| entry.first().and_then(ZrdValue::as_text) == Some("wingman_1"))
        .and_then(|entry| entry.get(1))
        .and_then(ZrdValue::as_list)
        .expect("the wingman_1 record list");
    assert_eq!(record.len(), 66);
    for field in [44_u64, 45, 46, 47, 48, 49] {
        let value = record
            .get(usize::try_from(field).expect("index"))
            .expect("the field exists");
        let ZrdValue::Float(number) = value else {
            panic!("field {field} is a float: {value:?}");
        };
        assert_eq!(
            number, &-1.0,
            "field {field} is the record's own unset slot"
        );
    }
}
