//! F13-D-FU2 (#1416): the four unjoined compiled anim event ids (13, 17, 26,
//! 28) and the engine's keyword for every stored id.
//!
//! Finding: `docs/findings/2026-10-18-f13-d-fu2-unjoined-anim-event-ids.md`.
//! Nothing here is `verified_original`.

use cs_app::animation::event_keywords::{
    ENGINE_EVENT_KEYWORDS, TwinEvidence, UNJOINED_EVENTS, engine_keyword, unjoined_event,
};
use cs_app::animation::events::{
    EventClass, EventStreamError, STORED_OPCODES, decode_event_stream, walk_event_stream,
};
use cs_app::animation::programs::read_animation_definition_member;
use cs_assets::install;
use cs_formats::io::ParseContext;
use cs_formats::script_raw::discover_container;
use cs_formats::zbd::{AnimationRecordSequenceKind, ZbdProbe, dispatch, read_animation_index};
use cs_types::install::RelativePath;
use std::collections::{BTreeMap, BTreeSet};

const UNJOINED: [u8; 4] = [13, 17, 26, 28];

fn retail_root() -> std::path::PathBuf {
    std::path::PathBuf::from(
        std::env::var("CS_GAME_DIR").expect("a retail test needs CS_GAME_DIR to be set"),
    )
}

/// One event block carrying only a header of opcode `op`.
fn header_only(op: u8) -> Vec<u8> {
    let mut bytes = Vec::new();
    bytes.extend_from_slice(&(u32::from(op) | 0x100).to_le_bytes());
    bytes.extend_from_slice(&8_u32.to_le_bytes());
    bytes
}

/// The table has one row for every id the event-stream table stores, none
/// twice, and each joined id carries the spelling F20 measured.
#[test]
fn accept_f13_d_fu2_table_has_a_row_for_every_stored_id() {
    let stored: Vec<u8> = STORED_OPCODES.iter().map(|info| info.opcode).collect();
    let rows: Vec<u8> = ENGINE_EVENT_KEYWORDS.iter().map(|row| row.opcode).collect();
    let mut stored = stored;
    stored.sort_unstable();
    assert_eq!(rows, stored, "one row per stored id, ascending");
    assert_eq!(rows.iter().collect::<BTreeSet<_>>().len(), 35);
    for info in STORED_OPCODES {
        let row = engine_keyword(info.opcode).expect("a row");
        assert!(!row.keyword.is_empty(), "{row:?}");
        assert!(row.writer_va >= 0x40_1000 && row.handler_va >= 0x40_1000);
        if info.class != EventClass::Unknown {
            assert_eq!(
                row.keyword, info.statement,
                "the engine's keyword agrees with F20's declaration join for id {}",
                info.opcode
            );
        }
    }
}

/// The four ids are named only in this table: the twin-join table still
/// carries them unjoined and decoding still refuses them, and removing a row
/// from (or silently naming an id in) either table fails here.
#[test]
fn accept_f13_d_fu2_unjoined_ids_stay_refused_and_unnamed_in_the_twin_table() {
    let unknown: Vec<u8> = STORED_OPCODES
        .iter()
        .filter(|info| info.class == EventClass::Unknown)
        .map(|info| info.opcode)
        .collect();
    assert_eq!(unknown, UNJOINED, "F20's unjoined ids");
    let described: Vec<u8> = UNJOINED_EVENTS.iter().map(|event| event.opcode).collect();
    assert_eq!(
        described, UNJOINED,
        "every unjoined id has a measured entry"
    );
    for event in UNJOINED_EVENTS {
        let info = STORED_OPCODES
            .iter()
            .find(|info| info.opcode == event.opcode)
            .expect("stored");
        assert!(
            info.statement.is_empty(),
            "{info:?} stays unnamed in the twin table"
        );
        assert_eq!(
            engine_keyword(event.opcode).expect("row").keyword,
            event.keyword
        );
        assert!(event.parameters.contains(&"START_TIME"), "{event:?}");
        assert!(!event.timing.is_empty() && !event.role.is_empty());
        let refusal = decode_event_stream(&header_only(event.opcode))
            .expect_err("the named id is still refused");
        assert!(
            matches!(refusal, EventStreamError::UnknownOpcode { opcode, .. } if opcode == event.opcode),
            "{refusal:?}"
        );
    }
    assert!(unjoined_event(11).is_none());
    assert_eq!(
        unjoined_event(28).expect("fog").twin,
        TwinEvidence::EngineOnly
    );
}

/// A declaration of the installation: archive, member, animation, sequences.
type Declaration = (String, String, String, Vec<(Option<String>, Vec<String>)>);
/// A carrier record: carrier, index, animation, every block as (is ordinary
/// sequence, name, event bytes).
type Record = (String, usize, String, Vec<(bool, String, Vec<u8>)>);

fn read_declarations(root: &std::path::Path, found: &install::Discovery) -> Vec<Declaration> {
    let mut decls = Vec::new();
    for file in &found.manifest.files {
        let key = file.relative_spelling.logical_key();
        if !key.ends_with("/zrdr.zbd") && key != "zbd/zrdr.zbd" {
            continue;
        }
        let bytes = std::fs::read(root.join(file.relative_spelling.as_str())).expect("read");
        let path =
            RelativePath::new(&file.relative_spelling.as_str().to_lowercase()).expect("path");
        for program in discover_container(&key, &path, &bytes).programs() {
            let Some(member) = program.locator().member().map(str::to_owned) else {
                continue;
            };
            let Ok(read) = read_animation_definition_member(&member, program.bytes()) else {
                continue;
            };
            for definition in read.definitions() {
                let Some(animation) = definition.animation_name() else {
                    continue;
                };
                let sequences = definition
                    .sequences()
                    .iter()
                    .map(|sequence| {
                        (
                            sequence.name().map(str::to_owned),
                            sequence
                                .entries()
                                .iter()
                                .map(|e| e.kind().to_owned())
                                .collect(),
                        )
                    })
                    .collect();
                decls.push((key.clone(), member.clone(), animation.to_owned(), sequences));
            }
        }
    }
    decls
}

fn read_records(root: &std::path::Path, found: &install::Discovery) -> Vec<Record> {
    let mut records = Vec::new();
    for file in &found.manifest.files {
        let key = file.relative_spelling.logical_key();
        if !(key.ends_with("/mis_anim.zbd") || key.ends_with("/cam_anim.zbd")) {
            continue;
        }
        let bytes = std::fs::read(root.join(file.relative_spelling.as_str())).expect("read");
        let path =
            RelativePath::new(&file.relative_spelling.as_str().to_lowercase()).expect("path");
        let mut context = ParseContext::with_defaults(&key);
        let decision = dispatch(ZbdProbe::new(&key, &path, &bytes[..8])).expect("a carrier");
        let index = read_animation_index(&mut context, decision, &bytes).expect("index");
        let walk = index
            .payload()
            .expect("payload")
            .records()
            .expect("records");
        for record in walk.iter() {
            records.push((
                key.clone(),
                record.index(),
                String::from_utf8_lossy(record.anim_name()).into_owned(),
                record
                    .sequences()
                    .iter()
                    .map(|block| {
                        (
                            block.kind() == AnimationRecordSequenceKind::Sequence,
                            String::from_utf8_lossy(block.name()).into_owned(),
                            block.events().to_vec(),
                        )
                    })
                    .collect(),
            ));
        }
    }
    records
}

/// Retail: every one of the stored events has the record length the engine's
/// parser stores for its id; the four ids' event counts match F20's census;
/// the reader-text twins of 13, 17 and 26 align at their ids (candidate
/// records, no other spelling) and no declaration statement carries
/// `FOG_STATE`.
#[test]
#[ignore = "requires CS_GAME_DIR: the original installation is needed"]
fn accept_f13_d_fu2_retail_lengths_counts_and_twin_alignment() {
    let root = retail_root();
    let found = install::discover(&root).expect("production discovery reads the installation");
    let decls = read_declarations(&root, &found);
    let records = read_records(&root, &found);
    assert!(!decls.is_empty() && !records.is_empty());

    let mut counts: BTreeMap<u8, u32> = BTreeMap::new();
    let mut total = 0_u32;
    for record in &records {
        for (_, name, bytes) in &record.3 {
            for event in walk_event_stream(bytes).expect("every block walks") {
                total += 1;
                *counts.entry(event.opcode).or_default() += 1;
                let row = engine_keyword(event.opcode).unwrap_or_else(|| {
                    panic!("{} {name}: id {} has no row", record.0, event.opcode)
                });
                if let Some(bytes) = row.record_bytes {
                    // Id 47 appends the SI script it reads to its fixed part.
                    let fits = if event.opcode == 47 {
                        event.length >= bytes
                    } else {
                        event.length == bytes
                    };
                    assert!(
                        fits,
                        "{} record {} {name}: id {} length {} against {bytes}",
                        record.0, record.1, event.opcode, event.length
                    );
                }
            }
        }
    }
    assert_eq!(total, 242_391, "F20's event census");
    assert_eq!(counts.len(), 35, "every stored id occurs and has a row");
    for event in UNJOINED_EVENTS {
        assert_eq!(counts[&event.opcode], event.events, "id {}", event.opcode);
    }

    // The twin alignment over every candidate record of a declaration.
    let mut join: BTreeMap<u8, BTreeMap<String, u32>> = BTreeMap::new();
    for decl in &decls {
        for record in records.iter().filter(|r| r.2 == decl.2) {
            let blocks: Vec<_> = record.3.iter().filter(|b| b.0).collect();
            if blocks.len() != decl.3.len() {
                continue;
            }
            let mut rows = Vec::new();
            let mut aligned = true;
            for (declared, block) in decl.3.iter().zip(&blocks) {
                let events = walk_event_stream(&block.2).expect("walks");
                let kinds: &[String] = if declared.1.len() == events.len() {
                    &declared.1
                } else if declared.1.len() == events.len() + 1 && declared.1[0] == "ACTIVATION" {
                    &declared.1[1..]
                } else {
                    aligned = false;
                    break;
                };
                rows.extend(
                    kinds
                        .iter()
                        .zip(&events)
                        .map(|(k, e)| (e.opcode, k.clone())),
                );
            }
            if aligned {
                for (op, kind) in rows {
                    *join.entry(op).or_default().entry(kind).or_default() += 1;
                }
            }
        }
    }
    for event in UNJOINED_EVENTS {
        match event.twin {
            TwinEvidence::CandidateAlignment { statements } => {
                let spellings = &join[&event.opcode];
                assert_eq!(
                    spellings,
                    &BTreeMap::from([(event.keyword.to_owned(), statements)]),
                    "id {} aligns to its keyword and no other spelling",
                    event.opcode
                );
            }
            TwinEvidence::EngineOnly => {
                assert!(!join.contains_key(&event.opcode), "id {}", event.opcode);
            }
            TwinEvidence::F20Join => unreachable!("an unjoined id"),
        }
    }
    let fog = decls
        .iter()
        .flat_map(|d| d.3.iter())
        .flat_map(|s| s.1.iter())
        .filter(|kind| *kind == "FOG_STATE")
        .count();
    assert_eq!(fog, 0, "no declaration statement carries FOG_STATE");
}

/// Engine image: every row's writer site stores the id (and the length), the
/// dispatch table registers the handler at the id's slot, and the keyword is
/// a NUL-delimited string of the image.
#[test]
#[ignore = "requires CS_ENGINE_IMAGE"]
fn accept_f13_d_fu2_engine_image_rows_match_the_image() {
    let path = std::env::var("CS_ENGINE_IMAGE").expect("CS_ENGINE_IMAGE must name the image");
    let image = std::fs::read(path).expect("read the image");
    let at = |va: u32| -> usize { (va - 0x40_0000) as usize };
    for row in ENGINE_EVENT_KEYWORDS {
        let site = &image[at(row.writer_va)..at(row.writer_va) + 24];
        // `mov byte ptr [reg(+0)], id`: C6 /0 with no displacement or disp8 0.
        let stores_id = match site[0..4] {
            [0xc6, 0x40..=0x47, 0x00, id] => id == row.opcode,
            [0xc6, 0x00..=0x07, id, _] => id == row.opcode,
            _ => false,
        };
        assert!(
            stores_id,
            "id {} writer {:#x}: {:02x?}",
            row.opcode, row.writer_va, site
        );
        if let Some(len) = row.record_bytes {
            let found = site.windows(7).any(|w| {
                w[0] == 0xc7
                    && (0x40..=0x47).contains(&w[1])
                    && w[2] == 4
                    && w[3..7] == len.to_le_bytes()
            });
            assert!(found, "id {} length {len}: {:02x?}", row.opcode, site);
        }
        let slot = 0x72_7de0 + 4 * u32::from(row.opcode);
        let mut registration = vec![0xc7, 0x05];
        registration.extend_from_slice(&slot.to_le_bytes());
        registration.extend_from_slice(&row.handler_va.to_le_bytes());
        let region = &image[at(0x4e_e000)..at(0x4e_e400)];
        let registered = region.windows(10).any(|w| w == registration.as_slice())
            // slot 0 / 46 style registrations go through a register
            || row.opcode == 46;
        assert!(
            registered,
            "id {} handler {:#x} at slot {slot:#x}",
            row.opcode, row.handler_va
        );
        let mut needle = vec![0_u8];
        needle.extend_from_slice(row.keyword.as_bytes());
        needle.push(0);
        assert!(
            image.windows(needle.len()).any(|w| w == needle.as_slice()),
            "keyword {} is a string of the image",
            row.keyword
        );
    }
}
