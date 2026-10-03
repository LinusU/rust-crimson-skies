//! F39-E3 acceptance: the objective-record census extended to the shared and
//! world-group readers.
//!
//! F39-D's census deliberately measured only mission-scoped reader archives.
//! This task measures the `zrdr.zbd` archives the mission-scope rule does not
//! claim — the shared reader (`zbd/zrdr.zbd`) and the world-group readers
//! (`zbd/<group>/zrdr.zbd`) — through the same production decode and the same
//! `objective_state_machine`, so the denominator's bound is measured rather
//! than assumed.
//!
//! The discriminating tests here call the census's production measurement on
//! authored documents (no installation needed); the retail test runs the real
//! census against `CS_GAME_DIR` and pins what the owner's installation
//! declares: every member of every non-mission reader decodes, no member
//! declares an `OBJECTIVE<N>` block, and exactly one member outside mission
//! scope carries an objective-record name — `zbd/c1c`'s `targets.zrd`.

use std::path::PathBuf;

use cs_app::objectives::{
    RetailObjectiveReaderRow, non_mission_reader_scope, reader_member_measurement,
    survey_retail_objective_records,
};
use cs_content::stunts::ZrdValue;
use cs_types::install::RelativePath;

/// A document in the wrapped flat-record shape of `objectives.zrd`: the root
/// is a one-element list holding one flat alternating record, whose
/// `OBJECTIVE<N>` fields each point at the block's own field list.
fn objectives_document(blocks: &[(&str, &[(&str, ZrdValue)])]) -> ZrdValue {
    let mut record = Vec::new();
    for (name, fields) in blocks {
        record.push(ZrdValue::Text((*name).to_owned()));
        let mut block = Vec::new();
        for (key, value) in *fields {
            block.push(ZrdValue::Text((*key).to_owned()));
            block.push(value.clone());
        }
        record.push(ZrdValue::List(block));
    }
    ZrdValue::List(vec![ZrdValue::List(record)])
}

/// A document in the `targets.zrd` shape: a list of records, each record a
/// list of `[name, value]` pairs.
fn targets_document(records: &[Vec<(&str, &str)>]) -> ZrdValue {
    ZrdValue::List(
        records
            .iter()
            .map(|record| {
                ZrdValue::List(
                    record
                        .iter()
                        .map(|(name, value)| {
                            ZrdValue::List(vec![
                                ZrdValue::Text((*name).to_owned()),
                                ZrdValue::Text((*value).to_owned()),
                            ])
                        })
                        .collect(),
                )
            })
            .collect(),
    )
}

#[test]
fn accept_f39_e3_the_non_mission_scope_rule_names_shared_and_world_group_readers() {
    // The scope rule is what the census partitions the installation's
    // `zrdr.zbd` archives with: mission scope stays with `mission_scope`, and
    // this rule owns the rest — the shared reader and the world-group
    // readers, nothing else.
    let shared = RelativePath::new("zbd/zrdr.zbd").expect("a path");
    assert_eq!(
        non_mission_reader_scope(&shared).as_deref(),
        Some("zbd"),
        "the shared reader must scope to `zbd`"
    );
    for group in ["c1", "c1b", "c1c", "c2", "c2b", "c3", "c4", "c5"] {
        let path = RelativePath::new(&format!("zbd/{group}/zrdr.zbd")).expect("a path");
        assert_eq!(
            non_mission_reader_scope(&path).as_deref(),
            Some(format!("zbd/{group}").as_str()),
            "{group}: the world-group reader must scope to its group"
        );
    }
    // Mission-scoped and unrelated `zrdr.zbd` paths belong to the mission
    // scope rule or to no reader at all — never to this bound.
    for key in [
        "zbd/c1/m02/zrdr.zbd",
        "zbd/c1/m02/deep/zrdr.zbd",
        "zbd/zzrdr.zbd",
        "other/zrdr.zbd",
    ] {
        let path = RelativePath::new(key).expect("a path");
        assert_eq!(
            non_mission_reader_scope(&path),
            None,
            "{key} is not a non-mission reader"
        );
    }
}

#[test]
fn accept_f39_e3_member_measurement_counts_objective_blocks_like_the_mission_rows() {
    // The bound is only meaningful if a block outside mission scope counts
    // the same as a block inside it: an authored `objectives.zrd`-shaped
    // document measured through the census's member measurement must report
    // its blocks and its complete in-block vocabulary — an implementation
    // that skipped the members would report zero here.
    let document = objectives_document(&[
        (
            "OBJECTIVE1",
            &[
                ("DZPATH1", ZrdValue::Int(7)),
                ("INACTIVE1", ZrdValue::Int(1)),
                ("WAKE_OBJECTIVE_WHEN_I_COMPLETE", ZrdValue::Int(2)),
            ],
        ),
        ("OBJECTIVE2", &[("TICK_DEPENDS_ON_OBJ", ZrdValue::Int(1))]),
    ]);
    let measured = reader_member_measurement("objectives.zrd", &document);
    assert_eq!(measured.blocks, 2, "the authored blocks were not counted");
    assert!(
        measured
            .keys
            .iter()
            .any(|(key, _)| key == "WAKE_OBJECTIVE_WHEN_I_COMPLETE"),
        "the in-block vocabulary was not published: {:?}",
        measured.keys
    );
    assert!(
        measured.mission_control,
        "objectives.zrd is a mission-control member name"
    );
    assert_eq!(measured.records, None, "targets records need targets.zrd");

    // A member that carries no OBJECTIVE<N> field measures to zero — the same
    // zero every shared- and world-group member measures on the installation.
    let plain = ZrdValue::List(vec![ZrdValue::List(vec![
        ZrdValue::Text("fog_zone".to_owned()),
        ZrdValue::Int(3),
    ])]);
    let quiet = reader_member_measurement("fogvol.zrd", &plain);
    assert_eq!(quiet.blocks, 0);
    assert!(quiet.keys.is_empty());
    assert!(!quiet.mission_control);
    assert_eq!(quiet.records, None);
}

#[test]
fn accept_f39_e3_a_targets_member_is_measured_as_objective_target_records() {
    // `targets.zrd` is the objective-record member outside the state-machine
    // blocks: measured through the production record readers it must report
    // its record count and its complete record-key vocabulary — the shape
    // `zbd/c1c/zrdr.zbd::targets.zrd` is measured with on the installation.
    let document = targets_document(&[
        vec![
            ("description", "MSG_OBJ_KLONDIKE"),
            ("category_label", "MSG_OBJ_ZEPPELIN"),
            ("help_label", "MSG_OBJ_DEFEND"),
        ],
        vec![
            ("description", "MSG_OBJ_DARKANGEL"),
            ("help_label", "MSG_OBJ_DISABLE"),
        ],
    ]);
    let measured = reader_member_measurement("targets.zrd", &document);
    assert!(
        measured.mission_control,
        "targets.zrd is a mission-control member name"
    );
    assert_eq!(measured.records, Some(2), "the authored records miscounted");
    assert_eq!(
        measured
            .record_keys
            .iter()
            .map(|(key, _)| key.as_str())
            .collect::<Vec<_>>(),
        vec!["category_label", "description", "help_label"],
        "the complete record-key vocabulary was not published"
    );
    assert_eq!(measured.blocks, 0, "a record list has no OBJECTIVE blocks");
}

#[test]
#[ignore = "requires CS_GAME_DIR"]
fn accept_f39_e3_retail_shared_and_world_group_readers_bound_the_denominator() {
    let game_dir = PathBuf::from(
        std::env::var("CS_GAME_DIR").expect("CS_GAME_DIR names the read-only installation"),
    );
    let census = survey_retail_objective_records(&game_dir).expect("the objective records survey");

    // The measured non-mission reader set: the shared reader plus the eight
    // world-group readers — exactly the `zrdr.zbd` archives the mission-scope
    // rule does not claim (F14-D.1's inventory).
    let scopes: Vec<&str> = census
        .readers()
        .iter()
        .map(|row: &RetailObjectiveReaderRow| row.scope.as_str())
        .collect();
    assert_eq!(
        scopes,
        vec![
            "zbd", "zbd/c1", "zbd/c1b", "zbd/c1c", "zbd/c2", "zbd/c2b", "zbd/c3", "zbd/c4",
            "zbd/c5"
        ],
        "the measured non-mission readers moved"
    );

    // Every member of every reader was decoded and measured — the searched
    // set, one row per index entry (the shared reader's index lists
    // `player.zrd` twice, so its 221 entries are 221 rows).
    let member_counts: Vec<(&str, usize)> = census
        .readers()
        .iter()
        .map(|row| (row.scope.as_str(), row.members.len()))
        .collect();
    assert_eq!(
        member_counts,
        vec![
            ("zbd", 221),
            ("zbd/c1", 70),
            ("zbd/c1b", 29),
            ("zbd/c1c", 28),
            ("zbd/c2", 70),
            ("zbd/c2b", 25),
            ("zbd/c3", 56),
            ("zbd/c4", 57),
            ("zbd/c5", 56),
        ],
        "the measured member set moved"
    );
    assert_eq!(census.reader_members(), 612);

    // The measured answer: no member of any non-mission reader declares an
    // `OBJECTIVE<N>` block, so the mission-scoped rows are the complete
    // denominator — a negative that is now proven, not assumed (F39-D's
    // unknown #5 resolved).
    for reader in census.readers() {
        for member in &reader.members {
            assert_eq!(
                member.blocks, 0,
                "{}::{} declares objective blocks the denominator cannot see",
                reader.scope, member.member
            );
        }
        assert_eq!(reader.blocks, 0);
        assert!(reader.keys.is_empty());
        assert_eq!(reader.container_sha256.len(), 64);
        for member in &reader.members {
            assert_eq!(member.member_sha256.len(), 64);
            assert!(
                member.member_len > 0,
                "{}::{} is empty",
                reader.scope,
                member.member
            );
        }
    }
    assert_eq!(census.reader_blocks(), 0);
    assert!(
        census.reader_vocabulary().is_empty(),
        "non-mission readers hold objective vocabulary: {:?}",
        census.reader_vocabulary()
    );

    // Exactly one member outside mission scope carries a mission-control
    // member name — `zbd/c1c`'s `targets.zrd`, the shared objective target
    // records C1C's missions can inherit — and its record measurement is
    // published with the complete key vocabulary.
    let inherited = census.inherited_members();
    assert_eq!(
        inherited.len(),
        1,
        "the inherited-member set moved: {inherited:?}"
    );
    let (scope, member) = inherited[0];
    assert_eq!(scope, "zbd/c1c");
    assert_eq!(member.member, "targets.zrd");
    assert_eq!(member.records, Some(5));
    assert_eq!(
        member.record_keys,
        vec![
            ("category_label".to_owned(), 3u32),
            ("description".to_owned(), 5u32),
            ("help_label".to_owned(), 5u32),
            ("nodes".to_owned(), 5u32),
            ("objective".to_owned(), 1u32),
        ],
        "the shared target records' vocabulary moved"
    );

    // The denominator itself is unchanged by the extension: 53 mission-scoped
    // readers, 1338 blocks — now measured-complete.
    assert_eq!(census.len(), 53);
    assert_eq!(census.blocks(), 1338);
    assert_eq!(census.install_sha256().len(), 64);
}
