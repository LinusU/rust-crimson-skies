//! Acceptance for Rally #1917 (`PARITY6-M01-AUDIO-TIMELINE.03`): the sound keys
//! of the M01 aircraft's `vehicle.zrd` records after their `kind_of` chains are
//! resolved, read from the retail installation by the production resolver.
//!
//! Finding: `docs/findings/2026-10-24-m01-audio-airframe-sound-tables.md`.
//! Task test prefix: `accept_parity6_m01_audio_03_`. The scenarios read
//! `CS_GAME_DIR` and fail loudly without it. Nothing here is `verified_original`.

use cs_content::original_airframe::{
    ResolvedVehicleRecord, SpannedDocument, resolve_vehicle_record,
};
use cs_content::stunts::ZrdValue;
use cs_formats::io::ParseContext;
use cs_formats::zbd::{ZbdProbe, dispatch, read_reader_archive, read_version_one_index};
use cs_types::evidence::ContentHash;

use super::accept_parity6_m01_audio_01_sample_identity::{
    archive_rows, game_dir, sound_records, sounds_zrd,
};

/// The vehicle records M01's aircraft resolve to (`2026-10-15-m01-aiv-layout.md`
/// section 5, plus the player's `pdevastator`).
const M01_RECORDS: [&str; 5] = [
    "pdevastator",
    "devastator",
    "wingman",
    "rusdevastator",
    "bsfury",
];

fn vehicle_document() -> SpannedDocument {
    let root = game_dir();
    let container = "ZBD/zrdr.zbd";
    let bytes = std::fs::read(root.join(container)).expect("zrdr.zbd readable");
    let spelling = cs_types::install::RelativePath::new(container).expect("relative path");
    let mut context = ParseContext::with_defaults("parity6_audio_03");
    let decision = dispatch(ZbdProbe::new(container, &spelling, &bytes)).expect("dispatch");
    let index = read_version_one_index(&mut context, decision, &bytes).expect("index");
    let table = index.member_table();
    let archive = read_reader_archive(&mut context, &table, index.data()).expect("archive");
    let entry = archive
        .entries()
        .find(|e| String::from_utf8_lossy(e.name()).eq_ignore_ascii_case("vehicle.zrd"))
        .expect("vehicle.zrd");
    let digest = ContentHash::from_hex(&"00".repeat(32)).expect("hash");
    SpannedDocument::decode(digest, "vehicle.zrd", entry.content(), 0, digest)
        .expect("vehicle.zrd decodes")
}

fn value(record: &ResolvedVehicleRecord, key: &str) -> Option<ZrdValue> {
    record.value(key).map(|v| v.to_zrd_value())
}

fn names(value: &ZrdValue) -> Vec<&str> {
    value
        .as_list()
        .expect("a list")
        .iter()
        .filter_map(ZrdValue::as_text)
        .collect()
}

#[test]
#[ignore = "requires CS_GAME_DIR"]
fn accept_parity6_m01_audio_03_every_m01_aircraft_inherits_the_damaged_engine_table() {
    let document = vehicle_document();
    let damaged = ZrdValue::List(vec![ZrdValue::List(vec![
        ZrdValue::Text("snd_damagedengine".to_owned()),
        ZrdValue::Float(0.0),
        ZrdValue::Float(1.0),
    ])]);
    for name in M01_RECORDS {
        let record = resolve_vehicle_record(&document, name).expect("resolves");
        // Only `basic_airplane` writes the key; the M01 records inherit it.
        assert_eq!(
            value(&record, "damaged_engine_sound"),
            Some(damaged.clone()),
            "{name}"
        );
        assert_eq!(
            record.source_of("damaged_engine_sound"),
            Some("basic_airplane"),
            "{name}"
        );
        assert_eq!(
            record.chain.first().map(String::as_str),
            Some("basic_airplane"),
            "{name}"
        );
        // No M01 aircraft has a prop voice source.
        assert_eq!(value(&record, "prop_sound"), None, "{name}");
    }
}

#[test]
#[ignore = "requires CS_GAME_DIR"]
fn accept_parity6_m01_audio_03_engine_and_cockpit_loops_per_m01_airframe() {
    let document = vehicle_document();
    let expect = [
        (
            "pdevastator",
            "snd_devastatorengine",
            "snd_devastator_cp",
            "pdevastator",
        ),
        (
            "devastator",
            "snd_devastatorengine",
            "snd_devastator_cp",
            "devastator",
        ),
        (
            "wingman",
            "snd_devastatorengine",
            "snd_devastator_cp",
            "devastator",
        ),
        (
            "rusdevastator",
            "snd_devastatorengine",
            "snd_devastator_cp",
            "devastator",
        ),
        ("bsfury", "snd_furyengine", "snd_fury_cp", "fury"),
    ];
    for (name, engine, cockpit, source) in expect {
        let record = resolve_vehicle_record(&document, name).expect("resolves");
        assert_eq!(
            names(&value(&record, "engine_sound").expect("engine_sound")),
            [engine]
        );
        assert_eq!(
            names(&value(&record, "cockpit_engine_sound").expect("cockpit")),
            [cockpit]
        );
        assert_eq!(record.source_of("engine_sound"), Some(source), "{name}");
    }
    // No record of the whole member names a prop sound, and exactly one
    // record (`basic_airplane`) writes the damaged-engine table.
    let text = format!("{:?}", document.root().to_zrd_value());
    assert_eq!(text.matches("prop_sound").count(), 0);
    assert_eq!(text.matches("damaged_engine_sound").count(), 1);
}

#[test]
#[ignore = "requires CS_GAME_DIR"]
fn accept_parity6_m01_audio_03_damaged_engine_member_is_not_the_unresolved_loop() {
    let root = game_dir();
    let mut records = Vec::new();
    sound_records(&sounds_zrd(&root), &mut records);
    let group = records
        .iter()
        .find(|r| r.0 == "snd_damagedengine")
        .expect("snd_damagedengine group");
    assert_eq!(group.1, "engine_damaged.wav");
    assert!(group.2.iter().any(|f| f == "LOOPED") && group.2.iter().any(|f| f == "3D"));
    let rows = archive_rows(&root, "ZBD/soundsh.zbd");
    let row = rows
        .iter()
        .find(|r| r.0 == "engine_damaged.wav")
        .expect("member");
    assert_eq!((row.2, row.3, row.4), (1, 22_050, 250_976));
    // Neither archive carries any member of the 33,864 B loop, engine or prop.
    for container in ["ZBD/soundsh.zbd", "ZBD/soundsl.zbd"] {
        assert!(
            archive_rows(&root, container).iter().all(|r| r.4 != 33_864),
            "{container}"
        );
    }
}
