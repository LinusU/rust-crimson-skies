//! Task #464 acceptance tests: what did the original pay for a stunt, and does
//! a second traversal pay again?
//!
//! Spec: `specs/F42-stunts-fame-photos-and-optional-achievement-events.md`,
//! stage `### F42-D`. Required capabilities: retail. Task test prefix:
//! `accept_f42_d_reward_`.
//!
//! The question has one half that files can answer and one that they cannot.
//! This file pins the half that files can answer, and pins it as a
//! **measurement with numbers behind it**, because a survey that answered
//! `reward_is_measured() == false` and nothing else would be indistinguishable
//! from a reader that never looked:
//!
//! * the objective **records** and the objective **state machine** carry a
//!   complete key vocabulary, and the payout/repeat scans are derived from it
//!   rather than asserted — an authored block that *does* carry a `payout` or a
//!   `repeatable` key must be found;
//! * the objective blocks that carry the original's own stunt completion
//!   condition (`DANGER_ZONES_COMPLETED`) carry a complete, closed key
//!   inventory, which is the surface a payout would have to appear on;
//! * the only numeric score table in the installation is the `score_*`
//!   multiplayer match table in the global reader's `player.zrd`, whose one
//!   negative-looking entry is the suicide penalty.
//!
//! The unignored tests author every byte (the `.zrd` grammar and a reader
//! archive) and run the production decoder, the production discovery and the
//! production survey. The `#[ignore]`d test reads the owner's installation
//! through the same production entry point and states the measured corpus.
//!
//! Nothing here is `verified_original`: no original run happened, and reading
//! the installation's files is not evidence of how the game behaves.

use std::fs;
use std::path::PathBuf;

use cs_app::stunts::{StuntRewardSurveyError, survey_retail_stunt_reward};
use cs_content::stunts::{
    OBJECTIVE_BLOCK_PREFIX, OBJECTIVE_DANGER_ZONE_COUNT_KEY, OBJECTIVE_DANGER_ZONES_KEY,
    REPEAT_KEY_VOCABULARY, REWARD_KEY_VOCABULARY, SCENARIO_OBJECTIVES_MEMBER,
    SCENARIO_TARGETS_MEMBER, SCORE_CONFIG_MEMBER, decode_zrd, objective_record_count,
    objective_record_keys, objective_state_machine, score_entries, vocabulary_keys,
};

// ------------------------------------------------------------- .zrd writer ---

/// A `.zrd` integer node: tag `1` and the value.
fn zrd_int(value: u32) -> Vec<u8> {
    let mut bytes = Vec::with_capacity(8);
    bytes.extend_from_slice(&1_u32.to_le_bytes());
    bytes.extend_from_slice(&value.to_le_bytes());
    bytes
}

/// A `.zrd` text node: tag `3`, the byte length and the bytes.
fn zrd_text(text: &str) -> Vec<u8> {
    let mut bytes = Vec::with_capacity(8 + text.len());
    bytes.extend_from_slice(&3_u32.to_le_bytes());
    bytes.extend_from_slice(&(text.len() as u32).to_le_bytes());
    bytes.extend_from_slice(text.as_bytes());
    bytes
}

/// A `.zrd` list node: tag `4`, then **`children.len() + 1`** as the count, then
/// the children (the measured `count - 1` grammar, F09/#463).
fn zrd_list(children: Vec<Vec<u8>>) -> Vec<u8> {
    let mut bytes = Vec::with_capacity(8 + children.iter().map(Vec::len).sum::<usize>());
    bytes.extend_from_slice(&4_u32.to_le_bytes());
    bytes.extend_from_slice(&((children.len() as u32) + 1).to_le_bytes());
    for child in children {
        bytes.extend_from_slice(&child);
    }
    bytes
}

/// A flat alternating key/value `.zrd` record, the shape `ia.zrd`,
/// `objectives.zrd` and `player.zrd` use.
fn zrd_flat(entries: Vec<(&str, Vec<u8>)>) -> Vec<u8> {
    let mut children = Vec::with_capacity(entries.len() * 2);
    for (key, value) in entries {
        children.push(zrd_text(key));
        children.push(value);
    }
    zrd_list(children)
}

/// One authored `targets.zrd` objective, a list of `[key, value]` pairs.
fn target_document(pairs: Vec<(&str, Vec<u8>)>) -> Vec<u8> {
    zrd_list(
        pairs
            .into_iter()
            .map(|(key, value)| zrd_list(vec![zrd_text(key), value]))
            .collect(),
    )
}

/// A one-element list of a text node, the shape the original writes a scalar in.
fn zrd_text1(text: &str) -> Vec<u8> {
    zrd_list(vec![zrd_text(text)])
}

/// One authored `objectives.zrd` block with the exact fields given.
fn objective_block(id: u32, entries: Vec<(&str, Vec<u8>)>) -> (String, Vec<u8>) {
    (format!("{OBJECTIVE_BLOCK_PREFIX}{id}"), zrd_flat(entries))
}

/// An authored `objectives.zrd`: the measured one-element wrapper around one
/// flat record.
fn objectives_document(blocks: Vec<(String, Vec<u8>)>) -> Vec<u8> {
    let mut entries: Vec<(&str, Vec<u8>)> = vec![
        ("MISSION_TIMER", zrd_list(vec![zrd_text("0.0")])),
        ("PLAYER_INIT", zrd_list(vec![zrd_int(1)])),
    ];
    for (key, body) in &blocks {
        entries.push((key.as_str(), body.clone()));
    }
    zrd_list(vec![zrd_flat(entries)])
}

/// One authored `player.zrd`-shaped score table: the measured one-element
/// wrapper around a flat record with the global `score_*` keys among unrelated
/// settings. The real member wraps its record the same way, so the fixture
/// exercises the wrapper the production reader must unwrap.
fn score_document(entries: Vec<(&str, u32)>) -> Vec<u8> {
    let mut fields: Vec<(&str, Vec<u8>)> = vec![
        ("smokescreen_stun_range", zrd_list(vec![zrd_int(600)])),
        ("respawn_rad", zrd_list(vec![zrd_int(1200)])),
    ];
    for (key, value) in entries {
        fields.push((key, zrd_list(vec![zrd_int(value)])));
    }
    zrd_list(vec![zrd_flat(fields)])
}

// ------------------------------------------------------- reader-archive writer ---

/// A version-one reader archive holding `members` in order, as task #463's test
/// authored it.
fn reader_archive(members: &[(&str, Vec<u8>)]) -> Vec<u8> {
    let mut bytes = Vec::new();
    let mut entries = Vec::with_capacity(members.len());
    for (name, member) in members {
        let start = bytes.len() as u32;
        bytes.extend_from_slice(member);
        entries.push((start, member.len() as u32, *name));
    }
    for (start, length, name) in &entries {
        bytes.extend_from_slice(&start.to_le_bytes());
        bytes.extend_from_slice(&length.to_le_bytes());
        let name = name.as_bytes();
        assert!(name.len() < 64, "a fixture member name fits its field");
        let mut field = [0_u8; 64];
        field[..name.len()].copy_from_slice(name);
        bytes.extend_from_slice(&field);
        bytes.extend_from_slice(&[0_u8; 76]);
    }
    bytes.extend_from_slice(&1_u32.to_le_bytes());
    bytes.extend_from_slice(&(members.len() as u32).to_le_bytes());
    bytes
}

/// A throwaway installation tree.
struct TempInstallation {
    root: PathBuf,
}

impl TempInstallation {
    fn new(label: &str) -> Self {
        let nanos = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .expect("the system clock is after the Unix epoch")
            .as_nanos();
        let root = std::env::temp_dir().join(format!(
            "crimson-t464-{label}-{}-{nanos}",
            std::process::id()
        ));
        fs::create_dir_all(&root).expect("the fixture root is created");
        Self { root }
    }

    fn write(&self, spelling: &str, bytes: &[u8]) {
        let path = self.root.join(spelling);
        fs::create_dir_all(path.parent().expect("a fixture spelling has a parent"))
            .expect("the fixture directories are created");
        fs::write(&path, bytes).expect("the fixture bytes are written");
    }
}

impl Drop for TempInstallation {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.root);
    }
}

// ------------------------------------------------ the extraction, on authored bytes ---

/// The score-table reader takes only the `score_*` keys of a flat record,
/// keeps the stored word verbatim and reads the suicide penalty's sign.
#[test]
fn accept_f42_d_reward_the_score_table_reads_only_score_keys_and_both_signs() {
    let document = decode_zrd(&score_document(vec![
        ("score_kill", 2),
        ("score_return_flag", 8),
        ("score_suicide", 4_294_967_294),
        ("score_zep", 10),
        ("score_enemy_flag", 10),
    ]))
    .expect("the authored score document decodes");

    let entries = score_entries(&document);
    let keys: Vec<&str> = entries.iter().map(|entry| entry.key()).collect();
    assert_eq!(
        keys,
        vec![
            "score_kill",
            "score_return_flag",
            "score_suicide",
            "score_zep",
            "score_enemy_flag",
        ],
        "only the `score_*` keys, in authored order, never the unrelated settings"
    );
    assert_eq!(entries[0].raw(), 2);
    assert_eq!(entries[0].signed(), 2);
    let suicide = entries
        .iter()
        .find(|entry| entry.key() == "score_suicide")
        .expect("the suicide entry is read");
    assert_eq!(suicide.raw(), 4_294_967_294, "the stored word is unsigned");
    assert_eq!(
        suicide.signed(),
        -2,
        "the signed reading is the documented reinterpretation of the word"
    );

    // A record with no `score_*` key is an empty table, not a zero.
    let unscore = decode_zrd(&score_document(Vec::new())).expect("the authored record decodes");
    assert!(
        score_entries(&unscore).is_empty(),
        "a record that carries no score key yields no entries"
    );
    assert!(
        score_entries(&decode_zrd(&zrd_text("not a record")).expect("a text node decodes"))
            .is_empty(),
        "a bare text node carries no table"
    );
}

/// The payout/repeat scan is derived from the complete key inventory, so an
/// authored `payout` or `repeatable` key is found rather than argued away, and
/// the match is exact: a substring such as `DANGER_ZONES_COMPLETION_COUNT` is
/// never read as a repeat count.
#[test]
fn accept_f42_d_reward_the_vocabulary_scan_finds_an_authored_payout_and_repeat_key() {
    let targets = decode_zrd(&zrd_list(vec![
        target_document(vec![
            ("description", zrd_text("MSG_TRGT_NYPD")),
            ("nodes", zrd_text1("dz1")),
            ("fame", zrd_list(vec![zrd_int(25)])),
        ]),
        target_document(vec![
            ("description", zrd_text("MSG_TRGT_FLAGBASE")),
            ("nodes", zrd_text1("ctf_1")),
            ("repeatable", zrd_list(vec![zrd_int(1)])),
        ]),
    ]))
    .expect("the authored targets decode");

    assert_eq!(objective_record_count(&targets), 2);
    let record_keys = objective_record_keys(&targets);
    assert_eq!(
        vocabulary_keys(&record_keys, &REWARD_KEY_VOCABULARY),
        vec!["fame"],
        "the authored reward key is found in the complete record inventory"
    );
    assert_eq!(
        vocabulary_keys(&record_keys, &REPEAT_KEY_VOCABULARY),
        vec!["repeatable"],
        "the authored repeat key is found"
    );

    // The block reader reports the complete inventory of a stunt block and the
    // payout/repeat keys derived from it.
    let machine = objective_state_machine(
        &decode_zrd(&objectives_document(vec![objective_block(
            1,
            vec![
                ("BEGIN_DORMANT", zrd_list(vec![zrd_int(1)])),
                (
                    OBJECTIVE_DANGER_ZONES_KEY,
                    zrd_list(vec![zrd_text("dzpath1")]),
                ),
                (OBJECTIVE_DANGER_ZONE_COUNT_KEY, zrd_list(vec![zrd_int(1)])),
                ("payout", zrd_list(vec![zrd_int(50)])),
                ("repeatable", zrd_list(vec![zrd_int(1)])),
            ],
        )]))
        .expect("the authored state machine decodes"),
    );

    assert_eq!(machine.stunt_conditions().len(), 1);
    let condition = &machine.stunt_conditions()[0];
    assert_eq!(condition.objective(), "OBJECTIVE1");
    let block_keys: Vec<&str> = condition
        .keys()
        .iter()
        .map(|(key, _)| key.as_str())
        .collect();
    assert_eq!(
        block_keys,
        vec![
            "BEGIN_DORMANT",
            "DANGER_ZONES_COMPLETED",
            "DANGER_ZONES_COMPLETION_COUNT",
            "payout",
            "repeatable",
        ],
        "the block's complete inventory, sorted, is the surface a payout must appear on"
    );
    assert_eq!(condition.reward_keys(), vec!["payout"]);
    assert_eq!(condition.repeat_keys(), vec!["repeatable"]);
    assert_eq!(condition.required_count(), Some(1));

    // Exact matching: a key that merely *contains* a vocabulary word is never a
    // hit. `score_kill` does not equal `score`, and
    // `DANGER_ZONES_COMPLETION_COUNT` does not equal `repeat_count`.
    let near_misses = vec![
        ("score_kill".to_owned(), 1_u32),
        (OBJECTIVE_DANGER_ZONE_COUNT_KEY.to_owned(), 1),
    ];
    assert!(
        vocabulary_keys(&near_misses, &REWARD_KEY_VOCABULARY).is_empty(),
        "a key that contains `score` is not a payout key"
    );
    assert!(
        vocabulary_keys(&near_misses, &REPEAT_KEY_VOCABULARY).is_empty(),
        "the zone completion count is not a repeat count"
    );
}

// -------------------------------------------------- the survey over a file ----

/// The whole production survey over an authored installation. The fixture
/// deliberately contains one authored payout key and one authored repeat key,
/// so that the survey's empty result over the owner's installation is a
/// measurement and not a hard-wired answer.
#[test]
fn accept_f42_d_reward_the_survey_measures_the_reward_surface_of_every_reader() {
    let install = TempInstallation::new("surface");
    // A reader that carries only the global score table.
    install.write(
        "ZBD/C5/IA1/zrdr.zbd",
        &reader_archive(&[(
            SCORE_CONFIG_MEMBER,
            score_document(vec![("score_kill", 2), ("score_suicide", 4_294_967_294)]),
        )]),
    );
    // A mission reader with objectives, an authored payout key and an authored
    // repeat key.
    install.write(
        "ZBD/C5/M01/zrdr.zbd",
        &reader_archive(&[
            (
                SCENARIO_TARGETS_MEMBER,
                zrd_list(vec![target_document(vec![
                    ("description", zrd_text("MSG_TRGT_NYPD")),
                    ("nodes", zrd_text1("dz1")),
                    ("repeatable", zrd_list(vec![zrd_int(1)])),
                ])]),
            ),
            (
                SCENARIO_OBJECTIVES_MEMBER,
                objectives_document(vec![objective_block(
                    1,
                    vec![
                        ("BEGIN_DORMANT", zrd_list(vec![zrd_int(1)])),
                        (
                            OBJECTIVE_DANGER_ZONES_KEY,
                            zrd_list(vec![zrd_text("dzpath1")]),
                        ),
                        ("payout", zrd_list(vec![zrd_int(50)])),
                    ],
                )]),
            ),
        ]),
    );
    // A reader with no objective member at all: a measured absence, not a
    // skipped row.
    install.write("ZBD/C5/M02/zrdr.zbd", &reader_archive(&[]));

    let survey = survey_retail_stunt_reward(&install.root)
        .unwrap_or_else(|error| panic!("surveys: {error}"));

    assert_eq!(survey.install_sha256().len(), 64);
    assert_eq!(survey.len(), 3, "every reader archive is a row");
    assert!(!survey.is_empty());
    assert_eq!(survey.objective_records(), 1);
    assert_eq!(survey.objective_blocks(), 1);

    // The payout/repeat scans are derived from the complete vocabulary and find
    // the authored keys, each named by its own row.
    assert_eq!(
        survey.reward_keys(),
        vec![("zbd/c5/m01/zrdr.zbd".to_owned(), "payout".to_owned())],
        "the authored payout key is found, so the measured corpus carrying none is a measurement"
    );
    assert_eq!(
        survey.repeat_keys(),
        vec![("zbd/c5/m01/zrdr.zbd".to_owned(), "repeatable".to_owned())]
    );

    // The stunt block's complete surface is reported.
    let block_key_inventory = survey.stunt_block_keys();
    let block_keys: Vec<&str> = block_key_inventory
        .iter()
        .map(|(key, _)| key.as_str())
        .collect();
    for expected in ["BEGIN_DORMANT", OBJECTIVE_DANGER_ZONES_KEY, "payout"] {
        assert!(
            block_keys.contains(&expected),
            "the stunt block surface has {expected}"
        );
    }

    // The score table is measured, with its provenance and sign.
    let table = survey
        .score_tables()
        .next()
        .expect("the global reader carries the score table");
    assert_eq!(table.span().member(), SCORE_CONFIG_MEMBER);
    assert_eq!(table.span().container(), "zbd/c5/ia1/zrdr.zbd");
    assert_eq!(table.span().container_sha256().len(), 64);
    assert!(table.span().length() > 0);
    assert_eq!(table.raw("score_kill"), Some(2));
    assert_eq!(table.raw("score_suicide"), Some(4_294_967_294));
    assert_eq!(table.raw("score_missing"), None);
    let entries: Vec<(&str, &str, i32)> = survey
        .score_entries()
        .map(|(container, entry)| (container, entry.key(), entry.signed()))
        .collect();
    assert_eq!(
        entries,
        vec![
            ("zbd/c5/ia1/zrdr.zbd", "score_kill", 2),
            ("zbd/c5/ia1/zrdr.zbd", "score_suicide", -2),
        ]
    );

    // The reader with no objective member is a measured absence, not a skipped
    // row.
    let empty = survey
        .rows()
        .iter()
        .find(|row| row.container() == "zbd/c5/m02/zrdr.zbd")
        .expect("the empty reader is a row");
    assert!(empty.objectives().is_none());
    assert!(empty.machine().is_none());
    assert!(empty.score().is_none());

    // The rule itself is unmeasured, and the survey says so instead of letting
    // "no payout key" stand in for an answer.
    assert!(!survey.reward_is_measured());
    assert!(!survey.repeat_is_measured());
}

/// The survey's own refusals, over files: an undecodable member and an
/// installation with no objective reader are each a **named** refusal rather
/// than a shorter row list.
#[test]
fn accept_f42_d_reward_every_reward_refusal_names_the_container_and_member() {
    let garbage = TempInstallation::new("garbage");
    garbage.write(
        "ZBD/C5/M01/zrdr.zbd",
        &reader_archive(&[
            (
                SCENARIO_OBJECTIVES_MEMBER,
                objectives_document(vec![objective_block(
                    1,
                    vec![(
                        OBJECTIVE_DANGER_ZONES_KEY,
                        zrd_list(vec![zrd_text("dzpath1")]),
                    )],
                )]),
            ),
            (SCENARIO_TARGETS_MEMBER, vec![0_u8; 12]),
        ]),
    );
    match survey_retail_stunt_reward(&garbage.root) {
        Err(StuntRewardSurveyError::Decode {
            container,
            member,
            code,
            ..
        }) => {
            assert_eq!(container, "zbd/c5/m01/zrdr.zbd");
            assert_eq!(member, SCENARIO_TARGETS_MEMBER);
            assert_eq!(code, "unknown_tag");
        }
        other => panic!("a non-.zrd member must be refused, got {other:?}"),
    }

    // A reader archive that carries the score table but no objectives at all:
    // the payout vocabulary scan would be empty for want of data.
    let scoreless = TempInstallation::new("scoreless");
    scoreless.write(
        "ZBD/C5/IA1/zrdr.zbd",
        &reader_archive(&[(SCORE_CONFIG_MEMBER, score_document(vec![("score_kill", 2)]))]),
    );
    assert!(
        matches!(
            survey_retail_stunt_reward(&scoreless.root),
            Err(StuntRewardSurveyError::NoObjectiveReaders)
        ),
        "a measurement with no objective in it must refuse rather than report nothing"
    );
}

// ------------------------------------------------------------------ retail ---

/// The retail root, or a loud failure. The test that calls this is `#[ignore]`d;
/// a test that skipped itself here would report a pass it never earned.
fn retail_root() -> PathBuf {
    PathBuf::from(std::env::var("CS_GAME_DIR").expect("CS_GAME_DIR is set for a retail test"))
}

/// The installation fingerprint of the owner's installation, recorded in
/// `docs/findings/2026-10-02-t464-stunt-reward-and-repeat.md`.
const RETAIL_INSTALL_SHA256: &str =
    "b4e780ab84cf31d85b8452fbfcec1478137768e32d9a75ccedc4c1847c631978";

/// The measured corpus, through the production survey.
///
/// Measured over the owner's installation: **62** reader archives walked,
/// **53** carrying **332** objective records and **1 338** numbered objective
/// blocks whose complete key vocabulary is **55** keys and **none** of which
/// names a payout or a repeat policy. The **31** blocks that carry the
/// original's own stunt completion condition (`DANGER_ZONES_COMPLETED`) carry a
/// closed **11**-key surface and no payout or repeat key. The only numeric score
/// table in the installation is the **5**-key `score_*` multiplayer match table
/// in the global reader's `player.zrd`.
#[test]
#[ignore = "requires CS_GAME_DIR"]
fn accept_f42_d_reward_retail_no_measured_file_records_a_stunt_payout_or_repeat() {
    let survey = survey_retail_stunt_reward(&retail_root()).expect("the retail corpus surveys");

    assert_eq!(
        survey.install_sha256(),
        RETAIL_INSTALL_SHA256,
        "the installation fingerprint the measurement was taken over"
    );
    assert_eq!(survey.len(), 62, "every reader archive in the installation");
    assert_eq!(survey.objective_records(), 332);
    assert_eq!(survey.objective_blocks(), 1_338);

    // No measured objective key names a payout or a repeat policy.
    let vocabulary = survey.objective_keys();
    let keys: Vec<(&str, u32)> = vocabulary
        .iter()
        .map(|(key, count)| (key.as_str(), *count))
        .collect();
    assert!(
        !keys
            .iter()
            .any(|(key, _)| REWARD_KEY_VOCABULARY.contains(key)),
        "no measured objective key names a payout: {keys:?}"
    );
    assert!(
        !keys
            .iter()
            .any(|(key, _)| REPEAT_KEY_VOCABULARY.contains(key)),
        "no measured objective key names a repeat policy: {keys:?}"
    );
    assert!(
        survey.reward_keys().is_empty(),
        "the survey's own payout scan agrees with the vocabulary"
    );
    assert!(survey.repeat_keys().is_empty());

    // The stunt completion blocks: a closed surface, and none of it pays.
    let conditions: Vec<_> = survey.stunt_conditions().collect();
    assert_eq!(conditions.len(), 31, "the original's stunt conditions");
    let block_keys = survey.stunt_block_keys();
    let block_key_names: Vec<&str> = block_keys.iter().map(|(key, _)| key.as_str()).collect();
    assert_eq!(
        block_key_names,
        vec![
            "ADD_OBJECTIVE_TARGET",
            "BEGIN_DORMANT",
            "COMPLETED_SOUND_GROUP",
            "DANGER_ZONES_COMPLETED",
            "DANGER_ZONES_COMPLETION_COUNT",
            "IDENTITY",
            "KILL_OBJECTIVE_WHEN_I_COMPLETE",
            "NAP_OBJECTIVE_WHEN_I_COMPLETE",
            "REMOVE_OBJECTIVE_TARGET",
            "SET_HELP_LABEL",
            "WAKE_OBJECTIVE_WHEN_I_COMPLETE",
        ],
        "the complete key surface of the 31 stunt blocks: not one key is a payout or a repeat rule"
    );
    for condition in &conditions {
        assert!(
            !condition.zones().is_empty(),
            "a stunt condition names at least one world zone"
        );
        assert!(
            condition.reward_keys().is_empty(),
            "{} names no payout",
            condition.objective()
        );
        assert!(
            condition.repeat_keys().is_empty(),
            "{} names no repeat policy",
            condition.objective()
        );
    }

    // The only numeric score table: the multiplayer `score_*` match table.
    let tables: Vec<_> = survey.score_tables().collect();
    assert_eq!(tables.len(), 1, "one reader carries a score table");
    let table = tables[0];
    assert_eq!(table.span().container(), "zbd/zrdr.zbd");
    assert_eq!(table.span().member(), SCORE_CONFIG_MEMBER);
    assert_eq!(
        table.keys(),
        vec![
            "score_kill",
            "score_return_flag",
            "score_suicide",
            "score_zep",
            "score_enemy_flag",
        ],
        "the complete score table, with no stunt, zone or photo entry"
    );
    assert_eq!(table.raw("score_kill"), Some(2));
    assert_eq!(table.raw("score_return_flag"), Some(8));
    assert_eq!(table.raw("score_suicide"), Some(4_294_967_294));
    assert_eq!(table.raw("score_zep"), Some(10));
    assert_eq!(table.raw("score_enemy_flag"), Some(10));

    // The rule itself remains unmeasured: the data names no payout and no
    // repeat policy, which is not the same as the original paying nothing.
    assert!(!survey.reward_is_measured());
    assert!(!survey.repeat_is_measured());
}
