//! Task #1415 (`F13-D-FU1`): the owner's anim-event keywords against F20's table.

use std::path::PathBuf;

use std::collections::BTreeSet;

use cs_app::animation::events::STORED_OPCODES;
use cs_app::animation::keywords::{OWNER_KEYWORDS, Placement, ReaderVocabulary, reconcile};
use cs_app::animation::programs::read_animation_definition_member;
use cs_assets::install;
use cs_formats::script_raw::discover_container;
use cs_types::install::RelativePath;

fn retail_vocabulary() -> ReaderVocabulary {
    let root =
        PathBuf::from(std::env::var("CS_GAME_DIR").expect("a retail test needs CS_GAME_DIR"));
    let found = install::discover(&root).expect("discovery reads the installation");
    let mut vocab = ReaderVocabulary::default();
    for file in &found.manifest.files {
        let key = file.relative_spelling.logical_key();
        if !key.ends_with("/zrdr.zbd") {
            continue;
        }
        let bytes = std::fs::read(root.join(file.relative_spelling.as_str())).expect("read");
        let path =
            RelativePath::new(&file.relative_spelling.as_str().to_lowercase()).expect("path");
        for program in discover_container(&key, &path, &bytes).programs() {
            let Some(member) = program.locator().member().map(str::to_owned) else {
                continue;
            };
            if let Ok(read) = read_animation_definition_member(&member, program.bytes()) {
                vocab.add_member(&read);
            }
        }
    }
    vocab
}

fn placed<'a>(r: &'a cs_app::animation::keywords::Reconciliation, label: &str) -> Vec<&'a str> {
    r.with_label(label).map(|row| row.keyword).collect()
}

/// Every name of both vocabularies is placed exactly once, with no reader text.
#[test]
fn accept_f13_d_fu1_every_name_is_placed_exactly_once() {
    let distinct: BTreeSet<_> = OWNER_KEYWORDS.iter().collect();
    assert_eq!(distinct.len(), OWNER_KEYWORDS.len(), "no owner name twice");
    let r = reconcile(&ReaderVocabulary::default());
    assert_eq!(r.owner.len(), 44);
    assert_eq!(placed(&r, "compiled_event_name").len(), 30);
    // The other 14 have no compiled spelling; with no reader text they are "neither".
    assert_eq!(placed(&r, "neither").len(), 14);
    // F20 side: 31 opcodes join an owner keyword, four join none, none is extra.
    assert_eq!(r.joined_opcodes.len(), 31);
    assert_eq!(r.unjoined_opcodes, vec![13, 17, 26, 28]);
    assert!(r.extra_f20.is_empty());
    assert_eq!(
        r.joined_opcodes.len() + r.unjoined_opcodes.len() + r.extra_f20.len(),
        STORED_OPCODES.len()
    );
    // 12 and 47 are the one spelling with two opcodes.
    let si = r
        .owner
        .iter()
        .find(|row| row.keyword == "OBJECT_MOTION_SI_SCRIPT")
        .unwrap();
    assert_eq!(
        si.placement,
        Placement::CompiledEventName {
            opcodes: vec![12, 47]
        }
    );
}

/// The reader text moves a keyword out of "neither", field before statement.
#[test]
fn accept_f13_d_fu1_reader_text_places_the_remaining_names() {
    let mut vocab = ReaderVocabulary::default();
    vocab.record_fields.insert("ACTIVATION".into(), 3);
    vocab.statement_kinds.insert("ACTIVATION".into(), 5);
    vocab.statement_kinds.insert("RESET_ANIMATION".into(), 2);
    vocab.statement_kinds.insert("generic".into(), 1);
    vocab.record_fields.insert("HEALTH".into(), 4);
    let r = reconcile(&vocab);
    assert_eq!(placed(&r, "reader_record_vocabulary_field"), ["ACTIVATION"]);
    assert_eq!(
        placed(&r, "reader_statement_without_stored_opcode"),
        ["RESET_ANIMATION"]
    );
    assert_eq!(placed(&r, "neither").len(), 12);
    assert_eq!(r.reader_statement_extras, vec![("generic".to_owned(), 1)]);
    assert_eq!(r.reader_field_extras, vec![("HEALTH".to_owned(), 4)]);
    let act = r
        .owner
        .iter()
        .find(|row| row.keyword == "ACTIVATION")
        .unwrap();
    assert_eq!(
        act.placement,
        Placement::ReaderRecordField {
            occurrences: 3,
            also_statement: 5
        }
    );
}

/// Retail: the placement the finding records.
#[test]
#[ignore = "requires CS_GAME_DIR"]
fn accept_f13_d_fu1_retail_placement() {
    let r = reconcile(&retail_vocabulary());
    assert_eq!(placed(&r, "compiled_event_name").len(), 30);
    assert_eq!(placed(&r, "reader_record_vocabulary_field"), ["ACTIVATION"]);
    assert_eq!(
        placed(&r, "reader_statement_without_stored_opcode"),
        [
            "OBJECT_OPACITY_STATE",
            "OBJECT_CYCLE_TEXTURE",
            "RESET_ANIMATION"
        ]
    );
    assert_eq!(
        placed(&r, "neither"),
        [
            "EFFECT",
            "FOG_STATE",
            "OBJECT_CONNECTOR",
            "OBJECT_VERTEX_COLOR",
            "CALL_OBJECT_CONNECTOR",
            "CAMERA_FROM_TO",
            "FBFX_CSINWAVE_FROM_TO",
            "ANIM_VERBOSE",
            "GLOBAL_ANIM_VERBOSE",
            "ON_CALL",
        ]
    );
    let act = r
        .owner
        .iter()
        .find(|row| row.keyword == "ACTIVATION")
        .unwrap();
    assert_eq!(
        act.placement,
        Placement::ReaderRecordField {
            occurrences: 1350,
            also_statement: 1914
        }
    );
    assert!(r.extra_f20.is_empty());
    assert_eq!(r.unjoined_opcodes, vec![13, 17, 26, 28]);
    let extras: Vec<_> = r
        .reader_statement_extras
        .iter()
        .map(|(n, c)| (n.as_str(), *c))
        .collect();
    assert_eq!(
        extras,
        [
            ("START_TIME", 13),
            ("generic", 1),
            ("instead", 1),
            ("is", 1)
        ]
    );
}
